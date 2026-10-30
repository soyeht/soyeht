import contextlib
import io
import re
import struct
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package_windows as package
import package_common as common

IMAGE_X64, IMAGE_ARM64 = 0x8664, 0xAA64


def pe_file(machine=IMAGE_X64, subsystem=package.CONSOLE):
    """The smallest header `pe_header` reads: a DOS stub that points at a PE header."""
    data = bytearray(0x200)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", data, 0x84, machine)
    struct.pack_into("<H", data, 0x80 + 24 + 68, subsystem)
    return bytes(data)


def fake_programs(directory, machine=IMAGE_X64, overrides=None):
    """One file per shipped program, standing in for the release build."""
    programs = {}
    for name in package.ENGINE_PROGRAMS + package.DESKTOP_PROGRAMS:
        path = Path(directory) / f"{name}.exe"
        subsystem = (overrides or {}).get(name, package.SUBSYSTEMS[name])
        path.write_bytes(pe_file(machine, subsystem))
        programs[name] = path
    return programs


def make_plan(programs=None, arch="x86_64", revision=0):
    return package.Plan("0.1.0", revision, arch, 1_700_000_000, programs or {})


class ProgramTests(unittest.TestCase):
    def test_the_header_names_the_processor_and_the_subsystem(self):
        self.assertEqual(package.pe_header(pe_file(IMAGE_ARM64, package.GUI)), (IMAGE_ARM64, package.GUI))
        with self.assertRaisesRegex(package.PackagingError, "MZ"):
            package.pe_header(b"\x7fELF" + bytes(100))
        broken = bytearray(pe_file())
        broken[0x80:0x84] = b"XXXX"
        with self.assertRaisesRegex(package.PackagingError, "PE header"):
            package.pe_header(bytes(broken))

    def test_programs_for_the_right_processor_and_subsystem_pass(self):
        with tempfile.TemporaryDirectory() as folder:
            programs = fake_programs(folder)
            package.check_programs(programs, make_plan(programs))

    def test_a_program_for_the_other_processor_stops_the_build(self):
        with tempfile.TemporaryDirectory() as folder:
            programs = fake_programs(folder, machine=IMAGE_ARM64)
            with self.assertRaisesRegex(package.PackagingError, "machine 0xAA64, not x86_64"):
                package.check_programs(programs, make_plan(programs, "x86_64"))
            package.check_programs(programs, make_plan(programs, "aarch64"))

    def test_a_window_that_would_open_a_console_stops_the_build(self):
        with tempfile.TemporaryDirectory() as folder:
            programs = fake_programs(folder, overrides={"soyeht-desktop": package.CONSOLE})
            with self.assertRaisesRegex(package.PackagingError, "soyeht-desktop.exe has PE subsystem 3"):
                package.check_programs(programs, make_plan(programs))

    def test_every_shipped_program_has_a_subsystem(self):
        shipped = set(package.ENGINE_PROGRAMS + package.DESKTOP_PROGRAMS)
        self.assertEqual(shipped, set(package.SUBSYSTEMS))
        # The window starts the engine and the supervisor beside itself (ADR 0102) and the
        # service host runs them for the tasks (ADR 0114): all five must travel with it.
        self.assertTrue({"soyeht-engine", "soyeht-ptyd", "soyeht-service-host", "soyeht"} <= shipped)


class NamingTests(unittest.TestCase):
    def test_every_name_of_the_installer_carries_next(self):
        package.check_names()
        self.assertEqual(package.APP_ID, "com.soyeht.next")
        self.assertEqual(package.INSTALL_FOLDER, "Soyeht Next")
        self.assertEqual(package.PRODUCT_NAME, "Soyeht")
        with patch.object(package, "APP_ID", "Soyeht"), self.assertRaises(SystemExit):
            package.check_names()

    def test_the_installer_is_named_for_the_update_manifest(self):
        self.assertEqual(make_plan().artifact(Path("dist")).name, "soyeht-0.1.0-windows-x86_64.exe")
        self.assertEqual(make_plan(arch="aarch64").artifact(Path("dist")).name, "soyeht-0.1.0-windows-aarch64.exe")

    def test_a_second_upload_of_one_version_sorts_after_it(self):
        self.assertEqual(make_plan(revision=0).display_version, "0.1.0")
        self.assertEqual(make_plan(revision=3).display_version, "0.1.0.3")
        self.assertEqual(make_plan(revision=0).file_version, "0.1.0.0")
        self.assertEqual(make_plan(revision=3).file_version, "0.1.0.3")

    def test_the_architectures_are_the_ones_the_manifest_knows(self):
        self.assertEqual(sorted(package.ARCHITECTURES), ["aarch64", "x86_64"])
        self.assertEqual(package.ARCHITECTURES["aarch64"][0], "aarch64-pc-windows-msvc")
        self.assertEqual(package.ARCHITECTURES["x86_64"][0], "x86_64-pc-windows-msvc")


class SigningTests(unittest.TestCase):
    def test_no_variable_means_an_unsigned_build(self):
        self.assertIsNone(package.signing_from({}))
        self.assertIsNone(package.signing_from({package.SIGN_THUMBPRINT: "  "}))

    def test_a_thumbprint_signs_with_the_certificate_of_the_store(self):
        signing = package.signing_from({package.SIGN_THUMBPRINT: "ABCDEF"})
        command = signing.command("signtool.exe", "a.exe")
        self.assertEqual(command[:2], ["signtool.exe", "sign"])
        self.assertEqual(command[-1], "a.exe")
        self.assertIn("/sha1", command)
        self.assertEqual(command[command.index("/sha1") + 1], "ABCDEF")
        self.assertEqual(command[command.index("/tr") + 1], package.TIMESTAMP_THUMBPRINT)
        self.assertEqual(command[command.index("/td") + 1], "SHA256")
        self.assertEqual(command[command.index("/fd") + 1], "SHA256")

    def test_azure_artifact_signing_needs_the_plugin_and_the_metadata(self):
        both = {package.SIGN_AZURE_DLIB: "C:/x/Azure.CodeSigning.Dlib.dll", package.SIGN_AZURE_METADATA: "m.json"}
        command = package.signing_from(both).command("signtool.exe")
        self.assertEqual(command[command.index("/dlib") + 1], "C:/x/Azure.CodeSigning.Dlib.dll")
        self.assertEqual(command[command.index("/dmdf") + 1], "m.json")
        self.assertEqual(command[command.index("/tr") + 1], package.TIMESTAMP_AZURE)
        for partial in ({package.SIGN_AZURE_DLIB: "d.dll"}, {package.SIGN_AZURE_METADATA: "m.json"}):
            with self.assertRaisesRegex(package.PackagingError, "needs both"):
                package.signing_from(partial)

    def test_two_kinds_of_certificate_are_refused(self):
        both = {package.SIGN_THUMBPRINT: "AB", package.SIGN_AZURE_DLIB: "d.dll"}
        with self.assertRaisesRegex(package.PackagingError, "set one kind"):
            package.signing_from(both)

    def test_the_timestamp_service_can_be_replaced(self):
        signing = package.signing_from({package.SIGN_THUMBPRINT: "AB", package.SIGN_TIMESTAMP: "http://t.example"})
        self.assertEqual(signing.timestamp, "http://t.example")

    def test_no_secret_is_ever_a_variable_of_this_script(self):
        # The certificate stays in a store or at the signing service; a password has no variable.
        names = {package.SIGN_THUMBPRINT, package.SIGN_AZURE_DLIB, package.SIGN_AZURE_METADATA, package.SIGN_TIMESTAMP}
        self.assertTrue(all(name.startswith("SOYEHT_WINDOWS_SIGN_") for name in names))
        self.assertFalse(any("PASSWORD" in name or "SECRET" in name or "TOKEN" in name for name in names))

    def test_inno_reads_the_signing_command_with_its_own_quoting(self):
        signing = package.Signing(("/sha1", "AB"), "http://t")
        option = package.inno_sign_option(signing, r"C:\Program Files (x86)\Kits\signtool.exe")
        self.assertTrue(option.startswith(f"/S{package.SIGN_NAME}=$qC:"))
        self.assertIn(r"signtool.exe$q sign /fd SHA256", option)
        self.assertTrue(option.endswith(" $f"))
        self.assertEqual(package.inno_quote("a$b c"), "$qa$$b c$q")

    def test_signtool_is_found_in_the_newest_sdk_for_the_host(self):
        with tempfile.TemporaryDirectory() as folder:
            kits = Path(folder) / "Windows Kits/10/bin"
            for version, architecture in (("10.0.19041.0", "x64"), ("10.0.26100.0", "arm64"), ("10.0.26100.0", "x64")):
                (kits / version / architecture).mkdir(parents=True)
                (kits / version / architecture / "signtool.exe").write_bytes(b"")
            environment = {"ProgramFiles(x86)": folder}
            found = package.find_signtool(None, environment, "aarch64")
            self.assertEqual(found, kits / "10.0.26100.0/arm64/signtool.exe")
            self.assertEqual(package.find_signtool(None, environment, "x86_64"), kits / "10.0.26100.0/x64/signtool.exe")
            self.assertEqual(package.find_signtool("mine.exe", environment, "x86_64"), Path("mine.exe"))
            self.assertEqual(package.find_signtool(None, {package.SIGNTOOL_PATH: "env.exe"}, "x86_64"), Path("env.exe"))
            with self.assertRaisesRegex(package.PackagingError, "signtool.exe was not found"):
                package.find_signtool(None, {"ProgramFiles(x86)": str(Path(folder) / "none")}, "x86_64")

    def test_an_arm_build_without_clang_stops_before_cargo_runs(self):
        with patch.object(package.shutil, "which", return_value=None):
            with self.assertRaisesRegex(package.PackagingError, "needs clang"):
                package.check_build_tools("aarch64")
            package.check_build_tools("x86_64")
        with patch.object(package.shutil, "which", return_value="clang.exe"):
            package.check_build_tools("aarch64")

    def test_the_unsigned_warning_cannot_be_missed(self):
        text = package.loud_unsigned()
        self.assertIn("UNSIGNED BUILD", text)
        self.assertIn("SOYEHT_WINDOWS_SIGN_", text)


class InstallerScriptTests(unittest.TestCase):
    script = package.SCRIPT.read_text()

    def arguments(self, signing=None):
        plan = make_plan(arch="aarch64", revision=2)
        return package.inno_arguments(plan, Path("stage"), Path("dist"), signing, "signtool.exe")

    def test_every_value_the_script_reads_is_given_to_it(self):
        used = set(re.findall(r"\{#(\w+)\}", self.script)) - {"Signed"}
        given = {argument[2:].split("=", 1)[0] for argument in self.arguments() if argument.startswith("/D")}
        self.assertEqual(used, given)

    def test_the_values_come_from_the_contracts(self):
        values = dict(argument[2:].split("=", 1) for argument in self.arguments() if argument.startswith("/D"))
        self.assertEqual(values["Version"], "0.1.0.2")
        self.assertEqual(values["VersionInfo"], "0.1.0.2")
        self.assertEqual(values["Allowed"], "arm64")
        self.assertEqual(values["Profile"], "prod")
        self.assertEqual(values["AppName"], package.PRODUCT_NAME)
        self.assertEqual(values["AppId"], "com.soyeht.next")
        self.assertEqual(values["OutputName"], "soyeht-0.1.0-windows-aarch64")
        self.assertEqual(self.arguments()[-1], str(package.SCRIPT))

    def test_signing_adds_the_command_and_the_define_only_when_asked(self):
        self.assertFalse(any("Signed" in argument or argument.startswith("/S") for argument in self.arguments()))
        signed = self.arguments(package.Signing(("/sha1", "AB"), "http://t"))
        self.assertIn("/DSigned=1", signed)
        self.assertTrue(any(argument.startswith(f"/S{package.SIGN_NAME}=") for argument in signed))

    def test_the_install_is_per_user_and_asks_for_no_administrator(self):
        self.assertIn("PrivilegesRequired=lowest", self.script)
        self.assertNotIn("PrivilegesRequiredOverridesAllowed", self.script)
        self.assertNotIn("{commonpf", self.script)
        self.assertNotIn("{autopf", self.script)
        self.assertNotIn("HKLM", self.script)
        self.assertIn(r"DefaultDirName={localappdata}\Programs\{#InstallFolder}", self.script)

    def test_login_tasks_are_registered_only_on_the_person_choice(self):
        # Contract 13, section 3: never silently. The box starts unchecked.
        self.assertRegex(self.script, r'Name: "loginstart";.*Flags: unchecked')
        self.assertEqual(self.script.count("RunCli('service install"), 1)
        self.assertIn("WizardIsTaskSelected('loginstart')", self.script)

    def test_an_upgrade_closes_only_the_window(self):
        # The supervisor holds the terminals (contract 07): the installer never closes it.
        self.assertIn("CloseApplicationsFilter=soyeht-desktop.exe", self.script)
        self.assertIn("RenameFile(", self.script)

    def test_the_uninstall_keeps_the_data_unless_the_person_chose_otherwise(self):
        self.assertEqual(self.script.count("--delete-data"), 1)
        self.assertIn("RunCli('uninstall')", self.script)
        self.assertIn("SwitchGiven('/DELETEDATA')", self.script)

    def test_the_command_line_is_given_the_profile_it_was_built_for(self):
        self.assertIn("'--profile ' + Profile", self.script)

    def test_the_script_names_no_machine_identifier_of_its_own(self):
        # The names come from the profile through the script; nothing is typed here twice.
        for typed in ("Soyeht.Next", "Soyeht.next", "Soyeht Next", "com.soyeht.next"):
            self.assertNotIn(typed, self.script)

    def test_the_product_name_is_typed_nowhere_in_the_script(self):
        lines = [line for line in self.script.splitlines() if not line.lstrip().startswith((";", "//"))]
        self.assertEqual([line for line in lines if "Soyeht" in line], [])

    def test_the_install_folder_is_checked_whatever_way_it_was_given(self):
        # /DIR= and a remembered folder get past the missing directory page: the folder is checked
        # at the start for /DIR= (silent: exit code 1, no message box) and before the first file.
        initialize = self.script[self.script.index("function InitializeSetup") :]
        initialize = initialize[: initialize.index("\nend;")]
        self.assertIn("{param:DIR|}", initialize)
        self.assertIn("FolderIsRefused(", initialize)
        prepare = self.script[self.script.index("function PrepareToInstall") :]
        self.assertIn("FolderIsRefused(ExpandConstant('{app}'))", prepare[: prepare.index("\nend;")])
        self.assertIn("{localappdata}\\Programs\\{#InstallFolder}", self.script)
        self.assertIn("DisableDirPage=yes", self.script)

    def test_only_the_programs_this_installer_ships_are_moved_deleted_or_ended(self):
        # No bare `*.exe` or `*.old`: a folder given by /DIR= may hold other programs.
        self.assertNotRegex(self.script, r"[\\']\*\.(exe|old)")
        self.assertNotIn("[InstallDelete]", self.script)
        self.assertIn("soyeht*.exe.*.old", self.script)
        self.assertEqual(self.script.count("FindFirst("), 2)
        self.assertEqual(self.script.count("'soyeht*.exe'") + self.script.count("'soyeht*.exe.*.old'"), 2)

    def test_the_programs_are_ended_by_a_script_that_reads_the_path_from_the_environment(self):
        # A quote in a user name must not break the script, and the 32-bit Setup must not use the
        # 32-bit PowerShell (it may not see the path of a 64-bit process).
        self.assertIn("{sysnative}", self.script)
        self.assertIn("Win32_Process", self.script)
        self.assertIn("SOYEHT_END_FOLDER", self.script)
        self.assertNotIn("$d = '''", self.script)

    def test_the_engine_is_ended_before_the_supervisor_and_the_programs_run_with_a_deadline(self):
        end = self.script[self.script.index("procedure EndProgramsOfTheFolder") :]
        self.assertLess(end.index("soyeht-engine*"), end.index("Live ''soyeht*''"))
        self.assertIn("RunWithin(Cli()", self.script)
        # A program that cannot start must not read as a success (the script would exit 0).
        self.assertIn("catch { exit 125 }", self.script)
        self.assertIn("WaitForExit(", self.script)

    def test_a_silent_run_never_waits_for_a_message_box(self):
        tell = self.script[self.script.index("procedure Tell") :]
        tell = tell[: tell.index("\nend;")]
        self.assertIn("WizardSilent()", tell)
        self.assertIn("UninstallSilent()", tell)
        self.assertNotIn("SuppressibleMsgBox('{#AppName} is installed", self.script)

    def test_a_failed_install_puts_the_programs_back(self):
        self.assertIn("RestoreMovedAside()", self.script[self.script.index("procedure DeinitializeSetup") :])
        post = self.script[self.script.index("else if CurStep = ssPostInstall") :]
        self.assertIn("InstallCompleted := True", post)

    def test_an_upgrade_compares_the_supervisor_protocol_before_ending_anything(self):
        self.assertIn("--protocol-version", self.script)
        settle = self.script[self.script.index("procedure SettleUpgrade") :]
        self.assertLess(settle.index("OldProtocol <> NewProtocol"), settle.index("EndProgramsOfTheFolder(False)"))


class BuildTests(unittest.TestCase):
    def test_the_staging_folder_holds_the_programs_and_the_licence(self):
        with tempfile.TemporaryDirectory() as folder:
            built = Path(folder) / "built"
            built.mkdir()
            programs = fake_programs(built)
            plan = make_plan(programs)
            staging = package.stage(plan, Path(folder) / "out")
            names = sorted(path.name for path in staging.iterdir())
            self.assertEqual(names, sorted([f"{name}.exe" for name in programs] + list(package.LICENSE_FILES)))

    def test_inno_is_found_by_argument_environment_or_the_usual_folders(self):
        with tempfile.TemporaryDirectory() as folder:
            usual = Path(folder) / "Programs/Inno Setup 6"
            usual.mkdir(parents=True)
            (usual / "ISCC.exe").write_bytes(b"")
            with patch.object(package.shutil, "which", return_value=None):
                self.assertEqual(package.find_inno(None, {"LOCALAPPDATA": folder}), usual / "ISCC.exe")
                self.assertEqual(package.find_inno("a.exe", {}), Path("a.exe"))
                self.assertEqual(package.find_inno(None, {package.INNO_PATH: "b.exe"}), Path("b.exe"))
                with self.assertRaisesRegex(package.PackagingError, "Inno Setup 6 was not found"):
                    package.find_inno(None, {})

    def run_main(self, folder, environment, *flags):
        built = Path(folder) / "built"
        built.mkdir()
        calls = []

        def build(engine, desktop, triple=None):
            calls.append(("cargo", triple))

        def find(engine, desktop, engine_names, desktop_names, triple=None, suffix=""):
            return fake_programs(built, IMAGE_ARM64 if triple.startswith("aarch64") else IMAGE_X64)

        def run(command, **keywords):
            command = [str(part) for part in command]
            calls.append(("run", command))
            if "ISCC.exe" in command[0]:
                name = next(a for a in command if a.startswith("/DOutputName=")).split("=", 1)[1]
                (Path(folder) / "out" / f"{name}.exe").write_bytes(b"installer")

        arguments = ["--out", str(Path(folder) / "out"), "--inno", "ISCC.exe", "--signtool", "signtool.exe", *flags]
        output = io.StringIO()
        with (
            patch.object(package, "cargo_build", build),
            patch.object(package.shutil, "which", return_value="clang.exe"),
            patch.object(package, "built_programs", find),
            patch.object(package, "run", run),
            patch.object(package, "stream", run),
            patch.object(common, "source_epoch", return_value=0),
            patch.dict(package.os.environ, environment, clear=True),
            contextlib.redirect_stdout(output),
            contextlib.redirect_stderr(output),
        ):
            package.main(arguments)
        return calls, output.getvalue()

    def test_both_architectures_are_built_and_each_gets_an_installer(self):
        with tempfile.TemporaryDirectory() as folder:
            calls, output = self.run_main(folder, {})
            self.assertEqual([c[1] for c in calls if c[0] == "cargo"], ["aarch64-pc-windows-msvc", "x86_64-pc-windows-msvc"])
            self.assertEqual(sum(1 for c in calls if c[0] == "run"), 2)
            self.assertTrue((Path(folder) / "out/soyeht-0.1.0-windows-x86_64.exe").is_file())
            self.assertTrue((Path(folder) / "out/soyeht-0.1.0-windows-aarch64.exe").is_file())
            self.assertIn("UNSIGNED BUILD", output)
            self.assertNotIn(".stage-windows", "".join(path.name for path in (Path(folder) / "out").iterdir()))

    def test_a_signed_build_signs_every_program_then_the_installer(self):
        with tempfile.TemporaryDirectory() as folder:
            calls, output = self.run_main(folder, {package.SIGN_THUMBPRINT: "AB"}, "--arch", "x86_64", "--skip-build")
            signed = [c[1][-1] for c in calls if c[0] == "run" and c[1][1:2] == ["sign"]]
            self.assertEqual(sorted(Path(path).name for path in signed), sorted(f"{n}.exe" for n in package.SUBSYSTEMS))
            verified = [c[1][-1] for c in calls if c[0] == "run" and c[1][1:2] == ["verify"]]
            self.assertEqual(len(verified), len(signed) + 1)
            self.assertTrue(verified[-1].endswith("soyeht-0.1.0-windows-x86_64.exe"))
            iscc = next(c[1] for c in calls if c[0] == "run" and c[1][0] == "ISCC.exe")
            self.assertIn("/DSigned=1", iscc)
            self.assertNotIn("UNSIGNED", output)
            self.assertNotIn("cargo", [c[0] for c in calls])

    def test_a_release_can_refuse_to_build_unsigned(self):
        with tempfile.TemporaryDirectory() as folder:
            with self.assertRaisesRegex(package.PackagingError, "--require-signature"):
                self.run_main(folder, {}, "--require-signature")


class SharedPiecesTests(unittest.TestCase):
    def test_the_two_platforms_name_a_package_by_one_rule(self):
        plan = common.Plan("1.2.3", 0, "aarch64", 0, {})
        self.assertEqual(plan.artifact_path("dist", "windows", "exe").name, "soyeht-1.2.3-windows-aarch64.exe")
        self.assertEqual(plan.artifact_path("dist", "linux", "deb").name, "soyeht-1.2.3-linux-aarch64.deb")

    def test_a_windows_host_reports_its_architecture_in_the_manifest_words(self):
        for machine, expected in (("ARM64", "aarch64"), ("AMD64", "x86_64"), ("x86_64", "x86_64")):
            with patch.object(common.platform, "machine", return_value=machine):
                self.assertEqual(common.host_arch(), expected)

    def test_a_target_triple_changes_where_the_programs_are_looked_for(self):
        with tempfile.TemporaryDirectory() as folder:
            release = Path(folder) / "x86_64-pc-windows-msvc/release"
            release.mkdir(parents=True)
            (release / "soyeht.exe").write_bytes(b"")
            found = common.built_programs(folder, folder, ("soyeht",), (), "x86_64-pc-windows-msvc", ".exe")
            self.assertEqual(found, {"soyeht": release / "soyeht.exe"})
            with self.assertRaisesRegex(common.PackagingError, "is missing"):
                common.built_programs(folder, folder, ("soyeht-engine",), (), "x86_64-pc-windows-msvc", ".exe")


class ReleaseEnvironmentTests(unittest.TestCase):
    """The release build of all three packagers (Mac, Linux, Windows) goes through one environment."""

    def flags(self, environment):
        return common.release_environment(environment)["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")

    def test_no_path_of_the_builder_is_left_to_the_compiler(self):
        environment = {"CARGO_HOME": "/b/cargo", "RUSTUP_HOME": "/b/rustup", "CARGO_TARGET_DIR": "/b/out"}
        flags = self.flags(environment)
        self.assertIn(f"--remap-path-prefix={common.ROOT}=/soyeht", flags)
        self.assertIn("--remap-path-prefix=/b/cargo=/cargo", flags)
        self.assertIn("--remap-path-prefix=/b/rustup=/rustup", flags)
        self.assertIn(f"--remap-path-prefix={Path('/b/out').resolve()}=/target", flags)
        self.assertIn(f"--remap-path-prefix={Path.home()}=/home", flags)

    def test_the_narrowest_rule_comes_last_since_the_last_matching_rule_wins(self):
        flags = self.flags({})
        home = flags.index(f"--remap-path-prefix={Path.home()}=/home")
        self.assertEqual(flags[-1], f"--remap-path-prefix={common.ROOT}=/soyeht")
        self.assertLess(home, len(flags) - 1)

    def test_flags_already_set_are_kept_and_a_path_with_a_blank_stays_one_flag(self):
        with patch.object(common.Path, "home", return_value=Path("/Users/Jane Smith")):
            flags = self.flags({"RUSTFLAGS": "-C target-cpu=native -D warnings"})
        self.assertEqual(flags[:4], ["-C", "target-cpu=native", "-D", "warnings"])
        self.assertIn("--remap-path-prefix=/Users/Jane Smith=/home", flags)
        encoded = common.release_environment({"CARGO_ENCODED_RUSTFLAGS": "-C\x1fdebuginfo=0"})
        self.assertTrue(encoded["CARGO_ENCODED_RUSTFLAGS"].startswith("-C\x1fdebuginfo=0\x1f--remap"))

    def test_the_production_profile_is_built_in_and_the_plain_variable_cannot_override_the_flags(self):
        result = common.release_environment({"RUSTFLAGS": "-C opt-level=1"})
        self.assertEqual(result[common.BUILD_ENVIRONMENT], common.PROFILE_NAME)
        self.assertNotIn("RUSTFLAGS", result)

    def test_both_workspaces_are_built_with_it(self):
        calls = []
        with patch.object(common, "stream", lambda command, environment=None, cwd=None: calls.append((command, environment))):
            common.cargo_build("/t/engine", "/t/desktop", "x86_64-pc-windows-msvc")
        self.assertEqual(len(calls), 2)
        for command, environment in calls:
            self.assertIn("--locked", command)
            self.assertIn(f"--remap-path-prefix={common.ROOT}=/soyeht", environment["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"))
        self.assertTrue(calls[0][1]["CARGO_TARGET_DIR"].endswith("engine"))

    def test_the_mac_and_linux_packagers_use_the_same_environment(self):
        import package_linux
        import package_mac

        self.assertIs(package_linux.cargo_build, common.cargo_build)
        calls = []
        with patch.object(package_mac, "stream", lambda command, environment=None, cwd=None: calls.append(environment)):
            package_mac.build_engine({})
        self.assertEqual(len(calls), 1)
        self.assertIn(f"--remap-path-prefix={common.ROOT}=/soyeht", calls[0]["CARGO_ENCODED_RUSTFLAGS"].split("\x1f"))


if __name__ == "__main__":
    unittest.main()
