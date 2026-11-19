import unittest
from unittest import mock

import e2e_swift_client as client

NAME = client.TEST_NAMES[0]
PASSED = f"""Test run started.
◇ Test {NAME}() started.
✔ Test {NAME}() passed after 4.2 seconds.
✔ Test run with 1 test in 0 suites passed after 4.2 seconds.
"""


class OutputTests(unittest.TestCase):
    def test_the_named_test_passing_once_is_a_pass(self):
        self.assertTrue(client.passed_once(PASSED, NAME))

    def test_a_filter_that_matched_nothing_is_not_a_pass(self):
        self.assertFalse(client.passed_once("Test run with 0 tests in 0 suites passed after 0.001 seconds.\n", NAME))
        self.assertFalse(client.passed_once("", NAME))

    def test_another_test_passing_is_not_a_pass(self):
        self.assertFalse(client.passed_once(PASSED.replace("nativeClient", "otherClient"), NAME))

    def test_a_test_that_ran_twice_is_not_a_pass(self):
        self.assertFalse(client.passed_once(PASSED + PASSED, NAME))

    def test_a_failing_test_is_not_a_pass(self):
        self.assertFalse(client.passed_once(PASSED + "✘ Test other() failed after 1 seconds.\n", NAME))


class CommandTests(unittest.TestCase):
    def test_it_runs_only_the_named_test_of_the_shared_package(self):
        command = client.swift_command(NAME)
        self.assertEqual(command[:2], ["swift", "test"])
        self.assertEqual(command[-2:], ["--filter", NAME])
        self.assertTrue(command[command.index("--package-path") + 1].endswith("apps/shared"))

    def test_each_test_runs_in_its_own_swift_run_and_a_failure_does_not_hide_the_next(self):
        with mock.patch.object(client, "run_test", side_effect=[1, 0]) as run, \
                mock.patch.object(client.e2e, "stage_programs", return_value="/staged"), \
                mock.patch.object(client.e2e, "require_binaries"), \
                mock.patch("sys.argv", ["e2e_swift_client.py"]):
            self.assertEqual(client.main(), 1)
        self.assertEqual([call.args[0] for call in run.call_args_list], list(client.TEST_NAMES))

    def test_it_starts_the_staged_programs_the_suites_start(self):
        with mock.patch.object(client.e2e, "stage_programs", return_value="/staged") as stage, \
                mock.patch.object(client.e2e, "require_binaries"), \
                mock.patch.object(client.e2e, "MACOS", True), \
                mock.patch.object(client, "TEST_NAMES", (NAME,)), \
                mock.patch.object(client.subprocess, "Popen") as popen, \
                mock.patch("sys.argv", ["e2e_swift_client.py"]):
            process = popen.return_value.__enter__.return_value
            process.stdout = iter(PASSED.splitlines(keepends=True))
            process.returncode = 0
            with mock.patch.object(client.sys, "stdout"):
                self.assertEqual(client.main(), 0)
        stage.assert_called_once()
        self.assertEqual(popen.call_args.kwargs["env"][client.BIN_DIR_ENV], "/staged")


if __name__ == "__main__":
    unittest.main()
