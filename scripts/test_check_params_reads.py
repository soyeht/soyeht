"""Regression tests for the untyped request parameter ratchet."""

from __future__ import annotations

import unittest
from pathlib import Path

from check_params_reads import KEY, main, sites
from count_ratchet_testing import fixture_root, run_main

SOURCE = '''
fn handler(params: &Value, request: &Request, other_params: &Value) {
    let one = params["cols"].as_u64();
    let two = request.params["toolCall"]["kind"] == "edit";
    let typed: ColsParams = serde_json::from_value(params.clone())?;
    let not_this = other_params["cols"];
    let text = "params[\\"cols\\"]";
    // params["cols"]
}

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod tests {
    fn helper(params: &Value) {
        let skipped = params["cols"];
    }
}
'''


class ParamsReadTests(unittest.TestCase):
    def fixture(self) -> Path:
        snippet = 'fn s() { params["x"]; }\n'
        return fixture_root(self, SOURCE, snippet, snippet)

    def test_counts_reads_by_key_in_engine_production_code(self):
        self.assertEqual(
            sites(self.fixture()),
            [("engine/soyeht-engine/src/lib.rs", 3), ("engine/soyeht-engine/src/lib.rs", 4)],
        )

    def test_the_count_may_not_grow_and_the_baseline_follows_it_down(self):
        for allowed, expected in ((2, 0), (1, 1), (3, 1)):
            with self.subTest(allowed=allowed):
                root = self.fixture()
                self.assertEqual(run_main(self, main, root, KEY, allowed), expected)


if __name__ == "__main__":
    unittest.main()
