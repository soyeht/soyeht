"""Regression tests for the standard error logging gate."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_logging import sites


LIBRARY = '''
fn run() {
    eprintln!("{}", json!({"event": "engine.x"}));
    eprint!("partial");
    let text = "eprintln!(not code)";
    // eprintln!("a comment")
}

#[cfg(test)]
mod tests {
    fn helper() {
        eprintln!("skipped: test only");
    }
}
'''


class LoggingGateTests(unittest.TestCase):
    def fixture(self) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        for crate in ("soyeht-engine", "soyeht-platform", "soyeht-cli"):
            (root / f"engine/{crate}/src").mkdir(parents=True)
            (root / f"engine/{crate}/tests").mkdir(parents=True)
        (root / "engine/soyeht-engine/src/lib.rs").write_text(LIBRARY)
        (root / "engine/soyeht-engine/src/main.rs").write_text('fn main() { eprintln!("e"); }\n')
        (root / "engine/soyeht-engine/tests/it.rs").write_text('fn t() { eprintln!("x"); }\n')
        (root / "engine/soyeht-platform/src/log.rs").write_text('fn w() { eprintln!("{}", 1); }\n')
        (root / "engine/soyeht-platform/src/other.rs").write_text('fn w() { eprintln!("{}", 1); }\n')
        (root / "engine/soyeht-cli/src/main.rs").write_text('fn main() { eprintln!("e"); }\n')
        (root / "apps/desktop/crates/ui/src").mkdir(parents=True)
        (root / "apps/desktop/crates/ui/examples").mkdir(parents=True)
        (root / "apps/desktop/crates/ui/src/app.rs").write_text('fn f() { eprintln!("soyeht: x"); }\n')
        (root / "apps/desktop/crates/ui/examples/tool.rs").write_text('fn main() { eprintln!("x"); }\n')
        return root

    def test_counts_hand_written_writes_outside_the_log_module_binaries_and_tests(self):
        found = sites(self.fixture())
        self.assertEqual(
            found,
            [
                ("engine/soyeht-engine/src/lib.rs", 3),
                ("engine/soyeht-engine/src/lib.rs", 4),
                ("engine/soyeht-platform/src/other.rs", 1),
                ("apps/desktop/crates/ui/src/app.rs", 1),
            ],
        )


if __name__ == "__main__":
    unittest.main()
