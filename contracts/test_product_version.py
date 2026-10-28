import unittest

from generated_header import header_text
from product_version import (
    RELEASE_SOURCE,
    SWIFT_OUTPUT,
    WINDOWS_OUTPUT,
    Release,
    Version,
    dump,
    load,
    parse,
    release,
    render,
    validate,
)


class VersionTests(unittest.TestCase):
    def test_a_version_is_three_plain_numbers(self):
        self.assertEqual(parse("1.20.3"), Version(1, 20, 3))
        for text in ("1.2", "1.2.3.4", "v1.2.3", "1.2.3-beta", "01.2.3", "1.-2.3", "", " 1.2.3", None):
            with self.assertRaises(ValueError, msg=repr(text)):
                parse(text)

    def test_a_part_stays_below_its_digits_of_the_build_number(self):
        parse("999.999.999")
        for text in ("1000.0.0", "0.1000.0", "0.0.1000"):
            with self.assertRaises(ValueError, msg=text):
                parse(text)

    def test_a_bump_resets_the_parts_below_it(self):
        version = Version(1, 2, 3)
        self.assertEqual(version.bumped("patch"), Version(1, 2, 4))
        self.assertEqual(version.bumped("minor"), Version(1, 3, 0))
        self.assertEqual(version.bumped("major"), Version(2, 0, 0))
        with self.assertRaises(ValueError):
            version.bumped("revision")


class BuildNumberTests(unittest.TestCase):
    def test_the_build_number_is_the_version_then_two_digits_of_revision(self):
        self.assertEqual(Release(Version(0, 1, 0)).build_number(), 100000)
        self.assertEqual(Release(Version(0, 1, 0), 7).build_number(), 100007)
        self.assertEqual(Release(Version(1, 2, 3), 4).build_number(), 100200304)

    def test_the_build_number_rises_with_the_version_then_the_revision(self):
        ordered = [
            release(parse(text), revision)
            for text, revision in (
                ("0.0.1", 0), ("0.0.1", 99), ("0.0.2", 0), ("0.1.0", 0), ("0.1.0", 1),
                ("0.9.9", 99), ("1.0.0", 0), ("1.0.1", 5), ("2.0.0", 0), ("21.0.0", 99),
            )
        ]
        numbers = [made.build_number() for made in ordered]
        self.assertEqual(numbers, sorted(set(numbers)))

    def test_a_new_version_starts_again_at_revision_zero(self):
        made = release(Version(1, 2, 3), 9)
        for kind in ("patch", "minor", "major"):
            self.assertEqual(made.bumped(kind).revision, 0, kind)
            self.assertGreater(made.bumped(kind).build_number(), made.build_number(), kind)

    def test_a_revision_keeps_the_version_and_raises_the_build_number_by_one(self):
        made = release(Version(1, 2, 3), 9).bumped("revision")
        self.assertEqual((made.version, made.revision), (Version(1, 2, 3), 10))
        self.assertEqual(made.build_number(), release(Version(1, 2, 3), 9).build_number() + 1)

    def test_the_revision_is_a_whole_number_from_0_to_99(self):
        release(Version(0, 1, 0), 99)
        with self.assertRaises(ValueError):
            release(Version(0, 1, 0), 99).bumped("revision")
        for revision in (-1, 100, 1.5, "1", True, None):
            with self.assertRaises(ValueError, msg=repr(revision)):
                release(Version(0, 1, 0), revision)

    def test_the_build_number_stays_below_2_to_the_31(self):
        # 20.999.999 is the last version that takes every revision; 21.474.836 revision 47 is the
        # last release of all (2**31 - 1 is 2147483647, and 21474836 * 100 + 47 is 2147483647).
        release(Version(20, 999, 999), 99)
        self.assertEqual(release(Version(21, 474, 836), 47).build_number(), 2**31 - 1)
        for version, revision in ((Version(21, 474, 836), 48), (Version(21, 474, 837), 0), (Version(21, 999, 999), 0), (Version(999, 0, 0), 0)):
            with self.assertRaises(ValueError, msg=f"{version} {revision}"):
                release(version, revision)

    def test_a_source_past_the_limit_is_refused(self):
        with self.assertRaises(ValueError):
            load('{"productVersion": "22.0.0"}')


class SourceTests(unittest.TestCase):
    def test_the_source_has_the_version_and_may_have_a_revision(self):
        self.assertEqual(load('{"productVersion": "0.1.0"}'), Release(Version(0, 1, 0), 0))
        self.assertEqual(load('{"productVersion": "0.1.0", "buildRevision": 3}'), Release(Version(0, 1, 0), 3))
        for document in ({}, {"productVersion": "0.1.0", "extra": 1}, {"version": "0.1.0"}, [], {"buildRevision": 1}):
            with self.assertRaises(ValueError, msg=repr(document)):
                validate(document)

    def test_the_source_is_written_and_read_back(self):
        made = release(Version(3, 4, 5), 6)
        self.assertEqual(load(dump(made)), made)

    def test_the_swift_constant_carries_the_header_and_the_version(self):
        output = render(release(Version(3, 4, 5), 6))[SWIFT_OUTPUT]
        self.assertTrue(output.startswith(f"// {header_text(RELEASE_SOURCE)}\n"))
        self.assertIn('public static let current = "3.4.5"', output)

    def test_the_windows_resource_gets_the_revision_for_its_fourth_field(self):
        output = render(release(Version(3, 4, 5), 6))[WINDOWS_OUTPUT]
        self.assertTrue(output.startswith(f"// {header_text(RELEASE_SOURCE)}\n"))
        self.assertIn("#define VERSION_REVISION 6\n", output)


if __name__ == "__main__":
    unittest.main()
