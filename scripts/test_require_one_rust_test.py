import unittest

import require_one_rust_test as check


class RequireOneRustTestTests(unittest.TestCase):
    def test_accepts_one_named_pass_and_rejects_a_filtered_out_name(self):
        expected = "terminal_capture::tests::measured"
        passing = (
            f"running 1 test\ntest {expected} ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out;\n"
        )
        filtered = (
            "running 0 tests\n"
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out;\n"
        )
        self.assertTrue(check.validates(passing, expected))
        self.assertFalse(check.validates(filtered, expected))

    def test_rejects_a_different_test_or_more_than_one_pass_summary(self):
        expected = "terminal_capture::tests::measured"
        other = (
            "running 1 test\ntest terminal_capture::tests::other ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out;\n"
        )
        duplicate = (
            f"running 1 test\ntest {expected} ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out;\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out;\n"
        )
        self.assertFalse(check.validates(other, expected))
        self.assertFalse(check.validates(duplicate, expected))


if __name__ == "__main__":
    unittest.main()
