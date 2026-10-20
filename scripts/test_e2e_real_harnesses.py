import unittest

from e2e_real_harnesses import Outcomes


class OutcomesTest(unittest.TestCase):
    def test_a_harness_with_a_step_and_no_failure_passed(self):
        outcomes = Outcomes(["a"])
        outcomes.harnesses["a"]["steps"]["replies"] = {"status": "passed"}
        self.assertEqual(outcomes.finish()["a"]["status"], "passed")

    def test_a_harness_that_ran_no_step_is_skipped_never_passed(self):
        done = Outcomes(["a"]).finish()["a"]
        self.assertEqual(done["status"], "skipped")
        self.assertEqual(done["reason"], "no step ran")

    def test_skipped_and_failed_harnesses_keep_their_status(self):
        outcomes = Outcomes(["a", "b"])
        outcomes.skip("a", "not signed in")
        outcomes.fail("b", "stop", "boom")
        done = outcomes.finish()
        self.assertEqual((done["a"]["status"], done["a"]["reason"]), ("skipped", "not signed in"))
        self.assertEqual((done["b"]["status"], done["b"]["error"]), ("failed", "stop: boom"))

    def test_only_a_pending_harness_is_alive(self):
        outcomes = Outcomes(["a", "b"])
        outcomes.skip("b", "x")
        self.assertTrue(outcomes.alive("a"))
        self.assertFalse(outcomes.alive("b"))


if __name__ == "__main__":
    unittest.main()
