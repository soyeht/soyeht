import contextlib
import io
import re
import struct
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package_windows as package
import packaging_common as common

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
        self.assertEqual(package.APP_ID, "Soyeht.Next")
        self.assertEqual(package.INSTALL_FOLDER, "Soyeht Next")
        with patch.object(package, "APP_ID", "Soyeht"), self.assertRaisesRegex(package.PackagingError, "no 'next'"):
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
        self.assertEqual(values["AppId"], "Soyeht.Next")
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
        for typed in ("Soyeht.Next", "Soyeht.next", "Soyeht Next"):
            self.assertNotIn(typed, self.script)


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
            calls.append(("run", [str(part) for part in command]))
            if "ISCC.exe" in str(command[0]):
                name = next(a for a in command if a.startswith("/DOutputName=")).split("=", 1)[1]
                (Path(folder) / "out" / f"{name}.exe").write_bytes(b"installer")

        arguments = ["--out", str(Path(folder) / "out"), "--inno", "ISCC.exe", "--signtool", "signtool.exe", *flags]
        output = io.StringIO()
        with (
            patch.object(package, "cargo_build", build),
            patch.object(package.shutil, "which", return_value="clang.exe"),
            patch.object(package, "built_programs", find),
            patch.object(package.subprocess, "run", run),
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


if __name__ == "__main__":
    unittest.main()
