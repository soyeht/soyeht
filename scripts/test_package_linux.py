import hashlib
import io
import lzma
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import xml.etree.ElementTree as ElementTree
from pathlib import Path
from unittest.mock import patch

import package_linux as package
from generated_profile import PROFILES

APP_RUN = package.PACKAGING / "AppRun"


def fake_programs(directory, content=b"program"):
    """One file per shipped program, standing in for the release build."""
    programs = {}
    for name in package.ENGINE_PROGRAMS + package.DESKTOP_PROGRAMS:
        path = directory / name
        path.write_bytes(content + name.encode())
        programs[name] = path
    return programs


def make_plan(programs, arch="x86_64", revision=0):
    return package.Plan("0.1.0", revision, arch, 1_700_000_000, programs)


def read_ar(data):
    """The members of an `ar` archive as {name: bytes}, checking the header of each."""
    assert data.startswith(b"!<arch>\n")
    members, position = {}, 8
    while position < len(data):
        header = data[position : position + 60]
        assert header[58:60] == b"`\n"
        size = int(header[48:58])
        members[header[:16].decode().strip()] = data[position + 60 : position + 60 + size]
        position += 60 + size + size % 2
    return members


def read_tar(data):
    return tarfile.open(fileobj=io.BytesIO(lzma.decompress(data)))


class InspectionTests(unittest.TestCase):
    def test_the_newest_glibc_symbol_version_is_the_floor(self):
        data = b"\0GLIBC_2.17\0GLIBC_2.34\0GLIBC_2.5\0GLIBC_PRIVATE\0GLIBCXX_3.4.30\0"
        self.assertEqual(package.glibc_floor(data), (2, 34))
        self.assertEqual(package.glibc_floor(b"static"), ())

    def test_the_linked_libraries_are_read_from_readelf(self):
        text = (
            " 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]\n"
            " 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n"
            " 0x000000000000000e (SONAME)             Library soname: [x.so]\n"
        )
        self.assertEqual(package.linked_libraries(text), ["libgcc_s.so.1", "libc.so.6"])

    def test_a_build_above_the_limit_is_refused_unless_any_is_allowed(self):
        package.check_glibc((2, 35), "2.35")
        package.check_glibc((2, 43), "any")
        with self.assertRaisesRegex(package.PackagingError, "glibc 2.43, above 2.35"):
            package.check_glibc((2, 43), "2.35")

    def test_a_library_nobody_declared_stops_the_build(self):
        with tempfile.TemporaryDirectory() as folder:
            programs = fake_programs(Path(folder))
            shown = subprocess.CompletedProcess([], 0, "(NEEDED) Shared library: [libsurprise.so.9]\n")
            with patch.object(package.subprocess, "run", return_value=shown):
                with self.assertRaisesRegex(package.PackagingError, "libsurprise.so.9"):
                    package.inspect_programs(programs)

    def test_dependencies_name_the_floor_and_what_the_window_loads(self):
        depends, recommends = package.dependencies((2, 35), {"libc.so.6", "libgcc_s.so.1"})
        self.assertTrue(depends.startswith("libc6 (>= 2.35), libgcc-s1, "))
        for needed in package.LOADED_REQUIRED:
            self.assertIn(needed, depends)
        self.assertIn("libwebkit2gtk-4.1-0", recommends)
        self.assertNotIn("libwebkit2gtk-4.1-0", depends)


class NamesTests(unittest.TestCase):
    def test_artifacts_are_named_after_the_version_and_the_architecture(self):
        plan = make_plan({}, arch="aarch64")
        out = Path("dist")
        self.assertEqual(plan.artifact(out, "deb").name, "soyeht-0.1.0-linux-aarch64.deb")
        self.assertEqual(plan.artifact(out, "AppImage").name, "soyeht-0.1.0-linux-aarch64.AppImage")
        self.assertEqual(plan.deb_arch, "arm64")
        self.assertEqual(make_plan({}).deb_arch, "amd64")

    def test_a_second_upload_of_a_version_sorts_after_the_first(self):
        self.assertEqual(make_plan({}).deb_version, "0.1.0")
        self.assertEqual(make_plan({}, revision=2).deb_version, "0.1.0-2")

    def test_the_package_is_for_the_production_profile(self):
        self.assertEqual(package.APPLICATION_ID, "com.soyeht.next")
        self.assertEqual(package.PROFILE_NAME, "prod")


class ShippedFilesTests(unittest.TestCase):
    def test_the_launcher_entry_starts_a_program_on_the_path_and_names_the_window(self):
        text = (package.PACKAGING / f"{package.APPLICATION_ID}.desktop").read_text()
        fields = dict(line.split("=", 1) for line in text.splitlines() if "=" in line and line[0] != "#")
        self.assertIn(fields["Exec"], package.ON_PATH)
        self.assertEqual(fields["StartupWMClass"], package.APPLICATION_ID)
        self.assertEqual(fields["Icon"], "soyeht")

    def test_the_metainfo_matches_the_application_id_and_gets_the_release_line(self):
        template = (package.PACKAGING / f"{package.APPLICATION_ID}.metainfo.xml").read_text()
        text = package.metainfo_text(template, make_plan({}))
        root = ElementTree.fromstring(text)
        self.assertEqual(root.findtext("id"), package.APPLICATION_ID)
        self.assertEqual(root.find("launchable").text, f"{package.APPLICATION_ID}.desktop")
        release = root.find("releases/release")
        self.assertEqual(release.attrib, {"version": "0.1.0", "date": "2023-11-14"})
        self.assertNotIn("0.1.0", template, "the version comes from contracts/release.json only")

    def test_a_template_without_the_marker_is_refused(self):
        with self.assertRaises(package.PackagingError):
            package.metainfo_text("<component/>", make_plan({}))

    def test_every_icon_size_exists(self):
        for size in package.ICON_SIZES:
            self.assertTrue((package.PACKAGING / f"icons/hicolor/{size}x{size}/apps/soyeht.png").is_file())

    def test_the_maintainer_scripts_are_valid_shell(self):
        scripts = [APP_RUN, package.PACKAGING / "deb/prerm", package.PACKAGING / "deb/refresh-caches"]
        for script in scripts:
            subprocess.run(["sh", "-n", str(script)], check=True)

    def test_the_folder_the_launcher_writes_is_named_after_the_production_data_directory(self):
        folder = PROFILES[package.PROFILE_NAME]["dataDirectory"]["linux"] + "-appimage"
        self.assertIn("next", folder)
        self.assertIn(f"/{folder}\n", APP_RUN.read_text())


class DebTests(unittest.TestCase):
    def build(self, directory, content=b"program", revision=0):
        programs = fake_programs(directory, content)
        plan = make_plan(programs, revision=revision)
        out = directory / "out"
        out.mkdir(exist_ok=True)
        path = package.build_deb(plan, (2, 35), {"libc.so.6", "libgcc_s.so.1"}, out)
        return path, out

    def test_the_archive_has_the_three_members_dpkg_reads(self):
        with tempfile.TemporaryDirectory() as folder:
            path, _ = self.build(Path(folder))
            self.assertEqual(path.name, "soyeht-0.1.0-linux-x86_64.deb")
            members = read_ar(path.read_bytes())
            self.assertEqual(list(members), ["debian-binary", "control.tar.xz", "data.tar.xz"])
            self.assertEqual(members["debian-binary"], b"2.0\n")

    def test_the_programs_sit_in_one_folder_and_two_of_them_are_on_the_path(self):
        with tempfile.TemporaryDirectory() as folder:
            path, _ = self.build(Path(folder))
            data = read_tar(read_ar(path.read_bytes())["data.tar.xz"])
            names = {member.name: member for member in data.getmembers()}
            for program in package.ENGINE_PROGRAMS + package.DESKTOP_PROGRAMS:
                member = names[f"./usr/lib/soyeht/{program}"]
                self.assertEqual(member.mode, 0o755)
            for program in package.ON_PATH:
                link = names[f"./usr/bin/{program}"]
                self.assertTrue(link.issym())
                self.assertEqual(link.linkname, f"../lib/soyeht/{program}")
            self.assertNotIn("./usr/bin/soyeht-engine", names)
            for needed in (
                "./usr/share/applications/com.soyeht.next.desktop",
                "./usr/share/metainfo/com.soyeht.next.metainfo.xml",
                "./usr/share/icons/hicolor/256x256/apps/soyeht.png",
                "./usr/share/doc/soyeht/copyright",
            ):
                self.assertIn(needed, names)
            self.assertTrue(all(m.uid == 0 and m.gid == 0 and m.uname == "root" for m in names.values()))
            self.assertTrue(all(m.mtime == 1_700_000_000 for m in names.values()))

    def test_the_control_file_declares_the_package_honestly(self):
        with tempfile.TemporaryDirectory() as folder:
            path, _ = self.build(Path(folder), revision=1)
            control = read_tar(read_ar(path.read_bytes())["control.tar.xz"])
            fields = dict(
                line.split(": ", 1)
                for line in control.extractfile("./control").read().decode().splitlines()
                if re.match(r"[A-Z][A-Za-z-]*: ", line)
            )
            self.assertEqual(fields["Package"], "soyeht")
            self.assertEqual(fields["Version"], "0.1.0-1")
            self.assertEqual(fields["Architecture"], "amd64")
            self.assertIn("libc6 (>= 2.35)", fields["Depends"])
            self.assertIn("libvulkan1", fields["Depends"])
            self.assertIn("curl", [name.strip() for name in fields["Depends"].split(",")])
            self.assertIn("libgtk-3-0t64 | libgtk-3-0", fields["Recommends"])
            self.assertGreater(int(fields["Installed-Size"]), 0)
            for script in ("postinst", "postrm", "prerm"):
                self.assertEqual(control.getmember(f"./{script}").mode, 0o755)

    def test_the_checksums_match_the_installed_files(self):
        with tempfile.TemporaryDirectory() as folder:
            path, _ = self.build(Path(folder))
            members = read_ar(path.read_bytes())
            data, control = read_tar(members["data.tar.xz"]), read_tar(members["control.tar.xz"])
            listed = dict(
                reversed(line.split("  ", 1)) for line in control.extractfile("./md5sums").read().decode().splitlines()
            )
            for name, digest in listed.items():
                content = data.extractfile("./" + name).read()
                self.assertEqual(hashlib.md5(content).hexdigest(), digest)
            self.assertIn("usr/lib/soyeht/soyeht-engine", listed)
            self.assertNotIn("usr/bin/soyeht", listed)

    def test_one_commit_builds_the_same_bytes_twice(self):
        with tempfile.TemporaryDirectory() as folder:
            first, _ = self.build(Path(folder))
            data = first.read_bytes()
            second, _ = self.build(Path(folder))
            self.assertEqual(data, second.read_bytes())

    def test_staging_leaves_nothing_behind(self):
        with tempfile.TemporaryDirectory() as folder:
            _, out = self.build(Path(folder))
            self.assertEqual([p.name for p in out.iterdir()], ["soyeht-0.1.0-linux-x86_64.deb"])


class StagingTests(unittest.TestCase):
    def test_modes_do_not_depend_on_the_builders_umask(self):
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            plan = make_plan(fake_programs(directory))
            previous = os.umask(0o077)
            self.addCleanup(os.umask, previous)
            for stage, name in ((package.stage_deb, "deb"), (package.stage_appimage, "image")):
                root = directory / name
                stage(root, plan)
                folders = [root, *(p for p in root.rglob("*") if p.is_dir() and not p.is_symlink())]
                self.assertTrue(all(p.stat().st_mode & 0o777 == 0o755 for p in folders), name)
                files = [p for p in root.rglob("*") if p.is_file() and not p.is_symlink()]
                self.assertTrue(all(p.stat().st_mode & 0o044 == 0o044 for p in files), name)

    def test_relative_targets_become_absolute(self):
        self.assertTrue(Path(package.absolute("target")).is_absolute())
        self.assertIsNone(package.absolute(None))


class AppImageTests(unittest.TestCase):
    def test_the_staged_image_has_the_entry_point_the_icon_and_a_build_id(self):
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            plan = make_plan(fake_programs(directory))
            root = directory / "AppDir"
            package.stage_appimage(root, plan)
            self.assertTrue(os.access(root / "AppRun", os.X_OK))
            self.assertTrue((root / "com.soyeht.next.desktop").is_file())
            self.assertEqual(os.readlink(root / ".DirIcon"), "soyeht.png")
            self.assertTrue((root / "usr/lib/soyeht/soyeht-desktop").is_file())
            self.assertEqual((root / "usr/lib/soyeht/BUILD_ID").read_text(), package.build_id(plan))

    def test_the_build_id_changes_with_the_programs_and_nothing_else(self):
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            plan = make_plan(fake_programs(directory))
            first = package.build_id(plan)
            self.assertEqual(first, package.build_id(plan))
            plan = make_plan(fake_programs(directory, b"another build"))
            self.assertNotEqual(first, package.build_id(plan))
            self.assertTrue(first.startswith("0.1.0+"))

    def test_the_runtime_is_checked_against_its_pinned_hash(self):
        with tempfile.TemporaryDirectory() as folder:
            runtime = Path(folder) / "runtime"
            runtime.write_bytes(b"not the runtime")
            with self.assertRaisesRegex(package.PackagingError, "not the pinned runtime"):
                package.runtime_file("x86_64", str(runtime), Path(folder))

    def test_a_download_is_checked_before_it_is_kept(self):
        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *arguments):
                return False

        with tempfile.TemporaryDirectory() as folder:
            cache = Path(folder) / "cache"
            with patch.object(package.urllib.request, "urlopen", return_value=Response(b"tampered")):
                with self.assertRaisesRegex(package.PackagingError, "not the pinned one"):
                    package.runtime_file("x86_64", None, cache)
            self.assertEqual(list(cache.iterdir()), [], "nothing unverified stays in the cache")
            body = b"the runtime"
            with patch.dict(package.ARCHITECTURES, {"x86_64": ("amd64", hashlib.sha256(body).hexdigest())}):
                with patch.object(package.urllib.request, "urlopen", return_value=Response(body)):
                    kept = package.runtime_file("x86_64", None, cache)
            self.assertEqual(kept, cache / "runtime-x86_64")
            self.assertEqual(kept.read_bytes(), body)
            self.assertEqual([p.name for p in cache.iterdir()], ["runtime-x86_64"])

    def test_both_architectures_have_a_pinned_runtime(self):
        for arch, (debian, digest) in package.ARCHITECTURES.items():
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
            self.assertTrue(debian)
            self.assertIn(arch, package.RUNTIME_URL.format(arch=arch))


class LauncherTests(unittest.TestCase):
    """AppRun, run for real against a stand-in image whose programs print how they were started."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.appdir = self.root / "mount"
        self.image("one")

    def image(self, build, soyeht_status=0):
        folder = self.appdir / "usr/lib/soyeht"
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "BUILD_ID").write_text(build + "\n")
        for name in ("soyeht-desktop", "soyeht-engine"):
            program = folder / name
            program.write_text(f'#!/bin/sh\necho "{name} $0 $*"\n')
            program.chmod(0o755)
        command = folder / "soyeht"
        command.write_text(f'#!/bin/sh\necho "soyeht $0 $*"\nexit {soyeht_status}\n')
        command.chmod(0o755)

    def run_launcher(self, *arguments):
        environment = {
            "PATH": os.environ["PATH"],
            "HOME": str(self.home),
            "APPDIR": str(self.appdir),
            "XDG_DATA_HOME": str(self.home / "data"),
        }
        return subprocess.run(
            ["sh", str(APP_RUN), *arguments], env=environment, capture_output=True, text=True
        )

    @property
    def programs(self):
        return self.home / "data/soyeht-next-appimage/programs"

    def test_the_window_runs_from_a_folder_that_never_moves(self):
        result = self.run_launcher()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), f"soyeht-desktop {self.programs}/soyeht-desktop")
        self.assertNotIn(str(self.appdir), result.stdout)

    def test_the_command_line_runs_from_the_same_folder_with_its_arguments(self):
        result = self.run_launcher("soyeht", "service", "status")
        self.assertEqual(result.stdout.strip(), f"soyeht {self.programs}/soyeht service status")

    def test_a_second_run_of_the_same_build_copies_nothing(self):
        self.run_launcher()
        marker = self.programs / "soyeht-engine"
        before = marker.stat().st_ino
        self.run_launcher()
        self.assertEqual(marker.stat().st_ino, before)

    def test_a_new_build_replaces_the_copy_and_leaves_no_leftovers(self):
        self.run_launcher()
        self.image("two")
        (self.appdir / "usr/lib/soyeht/soyeht-engine").write_text('#!/bin/sh\necho "engine two"\n')
        self.run_launcher()
        self.assertEqual((self.programs / "BUILD_ID").read_text(), "two\n")
        self.assertIn("engine two", (self.programs / "soyeht-engine").read_text())
        leftovers = [p.name for p in self.programs.parent.iterdir() if p.name != "programs"]
        self.assertNotIn("programs.old", " ".join(leftovers))
        self.assertFalse([name for name in leftovers if name.startswith("programs.")])
        self.assertFalse(list(self.programs.glob("programs*")), "no folder nested by a swap")

    @unittest.skipUnless(shutil.which("flock"), "the launcher cleans leftovers only under flock")
    def test_leftovers_of_a_launch_that_was_killed_are_removed(self):
        self.run_launcher()
        self.image("two")
        (self.programs.parent / "programs.new.99").mkdir()
        (self.programs.parent / "programs.old.99").mkdir()
        self.run_launcher()
        self.assertEqual(sorted(p.name for p in self.programs.parent.iterdir() if p.name != ".lock"), ["programs"])

    def test_uninstall_removes_the_copied_programs_only_when_it_succeeded(self):
        self.run_launcher()
        result = self.run_launcher("soyeht", "uninstall")
        self.assertEqual(result.returncode, 0)
        self.assertFalse(self.programs.parent.exists())
        self.image("one", soyeht_status=3)
        result = self.run_launcher("soyeht", "uninstall", "--delete-data")
        self.assertEqual(result.returncode, 3)
        self.assertTrue(self.programs.exists())

    def test_the_folder_is_the_persons_alone(self):
        self.run_launcher()
        self.assertEqual(self.programs.parent.stat().st_mode & 0o777, 0o700)


if __name__ == "__main__":
    unittest.main()
