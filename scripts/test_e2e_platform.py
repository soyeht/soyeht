import hmac
import json
import os
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

import e2e
import e2e_platform as platform
from generated_profile import ENGINE_SOCKET


def serve_loopback(listener, secret, outcome):
    """The listener side of contract 08, section 2.5."""
    connection, _ = listener.accept()
    with connection:
        client_nonce = platform.read_exact(connection, platform.NONCE_BYTES)
        server_nonce = os.urandom(platform.NONCE_BYTES)
        connection.sendall(server_nonce + platform.endpoint_proof(
            secret, "server", client_nonce, server_nonce))
        try:
            presented = platform.read_exact(connection, 32)
        except EOFError:
            outcome.append("dropped")
            return
        valid = hmac.compare_digest(
            presented, platform.endpoint_proof(secret, "client", client_nonce, server_nonce))
        outcome.append("accepted" if valid else "dropped")
        if valid:
            connection.sendall(b"hello")


class LaunchdDefinition(unittest.TestCase):
    def test_the_agent_is_rendered_by_the_service_command_and_not_by_this_script(self):
        with tempfile.TemporaryDirectory() as temporary:
            data = Path(temporary)
            cli = data / "soyeht"
            answers = [
                subprocess.CompletedProcess([], 0),  # the domain exists
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0, stdout=json.dumps({"text": "<plist/>"})),
            ]
            with mock.patch.object(platform.subprocess, "run", side_effect=answers) as run:
                launcher = platform.LaunchdLauncher(data, "com.soyeht.next.test", cli)
                launcher.define(
                    "engine", [Path("/x/soyeht-engine"), "--listen", "127.0.0.1:0"],
                    {"ZDOTDIR": "/z"}, data / "engine.log")
            command = run.call_args_list[2].args[0]
            self.assertEqual(command[:5], [str(cli), "service", "render", "--system", "macos"])
            self.assertEqual(command[command.index("--name") + 1], "com.soyeht.next.test.engine")
            self.assertIn("ZDOTDIR=/z", command)
            self.assertEqual(command[command.index("--") + 1:],
                             ["/x/soyeht-engine", "--listen", "127.0.0.1:0"])
            self.assertEqual((data / "engine.plist").read_text(), "<plist/>")
            self.assertEqual((data / "engine.plist").stat().st_mode & 0o777, 0o600)


class LaunchdBootstrap(unittest.TestCase):
    def launcher(self, directory):
        with mock.patch.object(platform.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)):
            return platform.LaunchdLauncher(Path(directory), "com.soyeht.next.test", Path(directory) / "soyeht")

    def failed(self, text):
        return subprocess.CompletedProcess([], 5, stdout="", stderr=text)

    def test_a_bootstrap_refused_while_launchd_removes_the_job_is_tried_again(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = self.launcher(directory)
            answers = [
                self.failed("Bootstrap failed: 5: Input/output error"),
                self.failed("Bootstrap failed: 37: Operation already in progress"),
                subprocess.CompletedProcess([], 0),
            ]
            with mock.patch.object(platform.subprocess, "run", side_effect=answers) as run, \
                    mock.patch.object(platform.time, "sleep") as sleep:
                launcher.start("engine")
            self.assertEqual(run.call_count, 3)
            self.assertEqual([call.args[0] for call in sleep.call_args_list], [0.2, 0.4])

    def test_any_other_failure_is_raised_at_once(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = self.launcher(directory)
            with mock.patch.object(platform.subprocess, "run",
                                   return_value=self.failed("Bootstrap failed: 113: Could not find")) as run, \
                    mock.patch.object(platform.time, "sleep") as sleep, \
                    self.assertRaises(subprocess.CalledProcessError):
                launcher.start("engine")
            self.assertEqual(run.call_count, 1)
            sleep.assert_not_called()

    def test_a_refusal_that_never_clears_is_raised_after_the_last_try(self):
        with tempfile.TemporaryDirectory() as directory:
            launcher = self.launcher(directory)
            with mock.patch.object(platform.subprocess, "run",
                                   return_value=self.failed("Bootstrap failed: 5: Input/output error")) as run, \
                    mock.patch.object(platform.time, "sleep"), \
                    self.assertRaises(subprocess.CalledProcessError):
                launcher.start("engine")
            self.assertEqual(run.call_count, len(platform.BOOTSTRAP_BACKOFF) + 1)


class LauncherKind(unittest.TestCase):
    def test_launchd_on_macos_with_launchctl_and_processes_elsewhere(self):
        self.assertEqual(platform.launcher_kind({}, "darwin", "/bin/launchctl"), "launchd")
        self.assertEqual(platform.launcher_kind({}, "darwin", ""), "process")
        self.assertEqual(platform.launcher_kind({}, "linux", ""), "process")
        self.assertEqual(platform.launcher_kind({}, "win32", ""), "process")

    def test_the_environment_forces_a_launcher(self):
        forced = {platform.LAUNCHER_ENV: "process"}
        self.assertEqual(platform.launcher_kind(forced, "darwin", "/bin/launchctl"), "process")
        forced = {platform.LAUNCHER_ENV: "launchd"}
        self.assertEqual(platform.launcher_kind(forced, "linux", "/bin/launchctl"), "launchd")

    def test_a_forced_launcher_must_exist(self):
        with self.assertRaises(AssertionError):
            platform.launcher_kind({platform.LAUNCHER_ENV: "launchd"}, "linux", "")
        with self.assertRaises(AssertionError):
            platform.launcher_kind({platform.LAUNCHER_ENV: "systemd"}, "linux", "/bin/launchctl")


class Paths(unittest.TestCase):
    def test_programs_get_the_exe_suffix_on_windows_only(self):
        with mock.patch.object(platform, "WINDOWS", True):
            self.assertEqual(platform.executable("soyeht-engine"), "soyeht-engine.exe")
        with mock.patch.object(platform, "WINDOWS", False):
            self.assertEqual(platform.executable("soyeht-engine"), "soyeht-engine")

    def test_data_roots_stay_short_for_unix_sockets(self):
        with mock.patch.object(platform, "WINDOWS", False):
            self.assertEqual(platform.short_temp_root(), Path("/tmp"))
        with mock.patch.object(platform, "WINDOWS", True):
            self.assertEqual(platform.short_temp_root(), Path(tempfile.gettempdir()))

    def test_the_cache_folder_follows_the_system(self):
        with tempfile.TemporaryDirectory() as home, mock.patch.object(Path, "home",
                                                                       return_value=Path(home)):
            clean = {key: value for key, value in os.environ.items()
                     if key not in ("XDG_CACHE_HOME", "LOCALAPPDATA")}
            with mock.patch.dict(os.environ, clean, clear=True):
                with mock.patch.object(platform, "MACOS", True), \
                        mock.patch.object(platform, "WINDOWS", False):
                    self.assertEqual(platform.user_cache_directory(),
                                     Path(home) / "Library/Caches")
                with mock.patch.object(platform, "MACOS", False), \
                        mock.patch.object(platform, "WINDOWS", True):
                    self.assertEqual(platform.user_cache_directory(),
                                     Path(home) / "AppData/Local")
                with mock.patch.object(platform, "MACOS", False), \
                        mock.patch.object(platform, "WINDOWS", False):
                    self.assertEqual(platform.user_cache_directory(), Path(home) / ".cache")
            with mock.patch.dict(os.environ, {"XDG_CACHE_HOME": f"{home}/xdg"}), \
                    mock.patch.object(platform, "MACOS", False), \
                    mock.patch.object(platform, "WINDOWS", False):
                self.assertEqual(platform.user_cache_directory(), Path(home) / "xdg")


class LauncherScripts(unittest.TestCase):
    def test_a_shell_script_exports_and_forwards_arguments(self):
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.object(platform, "WINDOWS", False):
            path = platform.write_launcher_script(
                Path(directory) / "run", {"KEY": "a b"}, ["/bin/echo", "x y"])
            self.assertEqual(path.name, "run")
            self.assertEqual(path.read_text(), "#!/bin/sh\nexport KEY='a b'\n"
                                               "exec /bin/echo 'x y' \"$@\"\n")
            self.assertTrue(os.access(path, os.X_OK))
            output = subprocess.run([str(path), "z"], capture_output=True, text=True).stdout
            self.assertEqual(output, "x y z\n")

    def test_windows_gets_a_cmd_file(self):
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.object(platform, "WINDOWS", True):
            path = platform.write_launcher_script(
                Path(directory) / "run", {"KEY": "50%"}, [r"C:\Program Files\node.exe", "a.js"])
            self.assertEqual(path.name, "run.cmd")
            self.assertEqual(path.read_bytes().decode(),
                             '@echo off\r\nset "KEY=50%%"\r\n'
                             '"C:\\Program Files\\node.exe" a.js %*\r\n')

    def test_a_cmd_value_cannot_hold_a_quote(self):
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.object(platform, "WINDOWS", True), self.assertRaises(AssertionError):
            platform.write_launcher_script(Path(directory) / "run", {"KEY": '"'}, ["a"])


class Endpoints(unittest.TestCase):
    def loopback(self, secret_for_file, secret_for_listener):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        listener = socket.socket()
        self.addCleanup(listener.close)
        listener.bind(("127.0.0.1", 0))
        listener.listen(1)
        path = Path(directory.name) / ENGINE_SOCKET
        path.write_text(f"{platform.ENDPOINT_MAGIC} {listener.getsockname()[1]} "
                        f"{secret_for_file.hex()}\n")
        outcome = []
        thread = threading.Thread(
            target=serve_loopback, args=(listener, secret_for_listener, outcome))
        thread.start()
        self.addCleanup(thread.join)
        return path, outcome, thread

    def test_the_endpoint_file_leads_to_a_proven_connection(self):
        secret = os.urandom(32)
        path, outcome, thread = self.loopback(secret, secret)
        with platform.connect_endpoint(path) as stream:
            self.assertEqual(stream.recv(5), b"hello")
        thread.join()
        self.assertEqual(outcome, ["accepted"])

    def test_a_listener_without_the_secret_is_refused(self):
        path, outcome, thread = self.loopback(os.urandom(32), os.urandom(32))
        with self.assertRaises(PermissionError):
            platform.connect_endpoint(path)
        thread.join()
        self.assertEqual(outcome, ["dropped"])

    def test_a_unix_socket_path_connects_directly(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as directory:
            path = Path(directory) / ENGINE_SOCKET
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(path))
                listener.listen(1)
                with platform.connect_endpoint(path) as stream:
                    peer, _ = listener.accept()
                    with peer:
                        stream.sendall(b"ping")
                        self.assertEqual(peer.recv(4), b"ping")

    def test_a_missing_endpoint_is_a_missing_file(self):
        with tempfile.TemporaryDirectory() as directory, self.assertRaises(FileNotFoundError):
            platform.connect_endpoint(Path(directory) / ENGINE_SOCKET)


class ProcessLauncherTest(unittest.TestCase):
    def test_start_stop_and_crash_restarts(self):
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory)
            launcher = platform.ProcessLauncher(data)
            launcher.define("engine", [sys.executable, "-c", "import time; time.sleep(60)"],
                            {"SOYEHT_MARK": "1"}, data / "engine.log")
            self.assertFalse(launcher.exists("engine"))
            launcher.start("engine")
            self.addCleanup(launcher.stop, "engine")
            self.assertTrue(launcher.exists("engine"))
            first = launcher.processes["engine"].pid
            launcher.crash("engine")
            self.assertTrue(launcher.exists("engine"))
            self.assertNotEqual(launcher.processes["engine"].pid, first)
            launcher.take_down("engine")
            self.assertFalse(launcher.exists("engine"))
            launcher.start("engine")
            launcher.stop("engine")
            launcher.wait_gone("engine")
            self.assertFalse(launcher.exists("engine"))


class DatabaseHelper(unittest.TestCase):
    def test_the_connection_is_committed_and_closed_on_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.db"
            with platform.database(path) as db:
                db.execute("CREATE TABLE item (value TEXT)")
                db.execute("INSERT INTO item VALUES ('kept')")
            with self.assertRaises(sqlite3.ProgrammingError):
                db.execute("SELECT 1")
            with platform.database(path) as other:
                self.assertEqual(other.execute("SELECT value FROM item").fetchall(), [("kept",)])

    def test_the_connection_is_closed_and_rolled_back_when_the_block_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.db"
            with platform.database(path) as db:
                db.execute("CREATE TABLE item (value TEXT)")
            with self.assertRaises(RuntimeError), platform.database(path) as db:
                db.execute("INSERT INTO item VALUES ('lost')")
                raise RuntimeError
            with self.assertRaises(sqlite3.ProgrammingError):
                db.execute("SELECT 1")
            with platform.database(path) as other:
                self.assertEqual(other.execute("SELECT count(*) FROM item").fetchone(), (0,))


class Teardown(unittest.TestCase):
    def test_a_sharing_violation_is_retried_for_a_bounded_time_on_windows(self):
        failures = [PermissionError(32, "in use")] * 2
        with mock.patch.object(platform, "WINDOWS", True), \
                mock.patch.object(platform.shutil, "rmtree", side_effect=[*failures, None]) as rmtree, \
                mock.patch.object(platform.time, "sleep"):
            platform.remove_directory(Path("data"))
        self.assertEqual(rmtree.call_count, 3)
        with mock.patch.object(platform, "WINDOWS", True), \
                mock.patch.object(platform.shutil, "rmtree", side_effect=PermissionError), \
                mock.patch.object(platform.time, "sleep"), \
                mock.patch.object(platform.time, "monotonic", side_effect=[0, 0, 11]), \
                self.assertRaises(PermissionError):
            platform.remove_directory(Path("data"))

    def test_other_systems_fail_at_the_first_error(self):
        with mock.patch.object(platform, "WINDOWS", False), \
                mock.patch.object(platform.shutil, "rmtree", side_effect=PermissionError) as rmtree, \
                self.assertRaises(PermissionError):
            platform.remove_directory(Path("data"))
        self.assertEqual(rmtree.call_count, 1)

    def test_the_folder_goes_only_after_every_service_has_exited(self):
        events = []
        running = {"engine", "ptyd"}

        class FakeLauncher:
            def exists(self, component):
                return component in running

            def stop(self, component):
                running.discard(component)
                events.append(f"stopped {component}")

        services = e2e.Services.__new__(e2e.Services)
        services.clients = []
        services.launcher = FakeLauncher()
        services.data = Path("data")
        services.close_supervised_processes = lambda: events.append("closed supervised")
        with mock.patch.object(e2e, "remove_directory",
                               lambda path: events.append(f"removed {path} with {sorted(running)}")):
            services.close()
        self.assertEqual(events, ["stopped engine", "closed supervised", "stopped ptyd",
                                  "removed data with []"])

    @unittest.skipIf(platform.WINDOWS, "the sweep is by process group on POSIX")
    def test_stopping_a_service_also_ends_what_it_started(self):
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory)
            child = data / "child.pid"
            script = ("import subprocess, sys, time\n"
                      "p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])\n"
                      f"open({str(child)!r}, 'w').write(str(p.pid))\n"
                      "time.sleep(60)\n")
            launcher = platform.ProcessLauncher(data)
            launcher.define("engine", [sys.executable, "-c", script], {}, data / "engine.log")
            launcher.start("engine")
            deadline = platform.time.monotonic() + 10
            while not child.exists() or not child.read_text():
                self.assertLess(platform.time.monotonic(), deadline)
                platform.time.sleep(0.05)
            pid = int(child.read_text())
            launcher.stop("engine")
            with self.assertRaises(ProcessLookupError):
                for _ in range(100):
                    os.kill(pid, 0)
                    platform.time.sleep(0.05)


class WaitEvent(unittest.TestCase):
    """`Client.wait_event` takes the next message of one method and keeps the rest."""

    def setUp(self):
        self.near, self.far = socket.socketpair()
        self.addCleanup(self.near.close)
        self.addCleanup(self.far.close)
        self.client = object.__new__(e2e.Client)
        self.client.stream = self.near
        self.client.pending = []

    def push(self, method, **params):
        e2e.send(self.far, {"jsonrpc": "2.0", "method": method, "params": params})

    def test_other_methods_are_skipped_and_kept_in_order(self):
        self.push("pane.updated", paneId="a")
        self.push("workspace.renamed", eventId=1)
        found = self.client.wait_event("workspace.renamed")
        self.assertEqual(found["params"]["eventId"], 1)
        self.assertEqual([m["method"] for m in self.client.pending], ["pane.updated"])

    def test_a_message_already_pending_is_found_without_reading(self):
        self.client.pending = [{"method": "x", "params": {}},
                               {"method": "y", "params": {"n": 2}}]
        self.assertEqual(self.client.wait_event("y", seconds=0)["params"], {"n": 2})
        self.assertEqual(self.client.pending, [{"method": "x", "params": {}}])

    def test_the_predicate_skips_a_message_of_the_right_method(self):
        self.push("pane.closed", paneId="other")
        self.push("pane.closed", paneId="mine")
        found = self.client.wait_event("pane.closed", lambda p: p["paneId"] == "mine")
        self.assertEqual(found["params"]["paneId"], "mine")
        self.assertEqual(self.client.pending[0]["params"]["paneId"], "other")

    def test_a_duplicate_of_the_method_is_the_next_match_not_skipped(self):
        self.push("workspace.renamed", eventId=5)
        self.push("workspace.renamed", eventId=5)
        first = self.client.wait_event("workspace.renamed")
        second = self.client.wait_event("workspace.renamed")
        self.assertEqual((first["params"]["eventId"], second["params"]["eventId"]), (5, 5))

    def test_a_timeout_names_the_method_and_the_skipped_methods(self):
        self.push("pane.updated")
        self.push("stream.gap")
        with self.assertRaises(AssertionError) as caught:
            self.client.wait_event("pane.closed", seconds=0.2, message="no close")
        text = str(caught.exception)
        for part in ("no close", "pane.closed", "pane.updated", "stream.gap", "2 message"):
            self.assertIn(part, text)
        self.assertEqual(len(self.client.pending), 2)
        self.assertIsNone(self.near.gettimeout())


class FakeServices:
    """What `run_suite` asks of the services: start two components, then close."""

    def __init__(self, close_error=None):
        self.events = []
        self.close_error = close_error

    def start(self, component):
        self.events.append(f"start {component}")

    def close(self):
        self.events.append("close")
        if self.close_error:
            raise self.close_error


class SuiteReport(unittest.TestCase):
    def run_suite(self, services, run, expected=2, setup=lambda: None):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "nested/report.json"
            code = None
            try:
                with mock.patch("builtins.print"):
                    e2e.run_suite(services, setup, run, expected, path)
            except SystemExit as exit_request:
                code = exit_request.code
            report = json.loads(path.read_text()) if path.is_file() else None
        return code, report

    def test_a_run_with_every_result_passes_and_writes_its_report(self):
        services = FakeServices()
        code, report = self.run_suite(
            services, lambda results: results.extend([{"name": "a"}, {"name": "b"}]))
        self.assertIsNone(code)
        self.assertTrue(report["passed"])
        self.assertEqual([item["name"] for item in report["results"]], ["a", "b"])
        self.assertNotIn("error", report)
        self.assertEqual(services.events, ["start ptyd", "start engine", "close"])

    def test_a_missing_result_fails_the_run(self):
        code, report = self.run_suite(FakeServices(), lambda results: results.append({}))
        self.assertEqual(code, 1)
        self.assertFalse(report["passed"])

    def test_a_failure_names_its_type_and_still_closes_the_services(self):
        services = FakeServices()

        def run(results):
            raise AssertionError("no pane")

        code, report = self.run_suite(services, run)
        self.assertEqual(code, 1)
        self.assertEqual(report["error"], "AssertionError: no pane")
        self.assertEqual(services.events[-1], "close")

    def test_a_failed_setup_stops_before_the_services_start(self):
        services = FakeServices()

        def setup():
            raise OSError("no binary")

        code, report = self.run_suite(services, lambda results: None, setup=setup)
        self.assertEqual(code, 1)
        self.assertEqual(report["error"], "OSError: no binary")
        self.assertEqual(services.events, ["close"])

    def test_a_failed_cleanup_is_reported_over_a_passing_run(self):
        services = FakeServices(close_error=RuntimeError("still running"))
        code, report = self.run_suite(
            services, lambda results: results.extend([{}, {}]))
        self.assertEqual(code, 1)
        self.assertEqual(report["error"], "cleanup failed: still running")

    def test_a_report_without_a_path_is_only_printed(self):
        with mock.patch("builtins.print") as printed:
            e2e.finish_report({"passed": True}, None)
        printed.assert_called_once()
        with self.assertRaises(SystemExit):
            with mock.patch("builtins.print"):
                e2e.finish_report({"passed": False}, None)

    def test_a_timed_report_passes_unless_it_holds_an_error(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            with mock.patch("builtins.print"):
                e2e.finish_timed_report({}, e2e.time.monotonic(), path)
                self.assertTrue(json.loads(path.read_text())["passed"])
                with self.assertRaises(SystemExit):
                    e2e.finish_timed_report(
                        {"error": "x"}, e2e.time.monotonic(), path)
            self.assertFalse(json.loads(path.read_text())["passed"])

    def test_the_binaries_are_found_by_their_platform_file_names(self):
        with tempfile.TemporaryDirectory() as directory:
            binaries = Path(directory)
            (binaries / platform.executable("soyeht")).write_text("")
            e2e.require_binaries(binaries, ("soyeht",), "present")
            with self.assertRaises(AssertionError):
                e2e.require_binaries(binaries, ("soyeht", "soyeht-engine"), "missing")

    def test_the_parser_takes_the_binary_folder_and_the_report(self):
        arguments = e2e.argument_parser("doc").parse_args(
            ["--bin-dir", "x", "--report", "y"])
        self.assertEqual((arguments.bin_dir, arguments.report), (Path("x"), Path("y")))
        defaults = e2e.argument_parser("doc").parse_args([])
        self.assertEqual(defaults.bin_dir, e2e.ROOT / "engine/target/debug")
        self.assertIsNone(defaults.report)


if __name__ == "__main__":
    unittest.main()
