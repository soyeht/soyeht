import unittest

import check_editor_performance as gate


class EditorPerformanceGateTests(unittest.TestCase):
    def test_green_control_accepts_every_budget_below_its_limit(self):
        measurements = {
            "open_100k_ms": 149.999,
            "keystroke_to_pixel_p95_ms": 15.999,
            "scroll_frame_p95_ms": 16.666,
            "search_1gb_ms": 999.999,
            "scroll_frames": 60,
            "editor_render_ink_pixels": 500,
        }
        self.assertEqual(gate.failures(measurements), [])

    def test_red_control_rejects_each_limit_and_too_few_frames(self):
        measurements = {
            "open_100k_ms": 150.0,
            "keystroke_to_pixel_p95_ms": 16.0,
            "scroll_frame_p95_ms": 1000.0 / 60.0,
            "search_1gb_ms": 1000.0,
            "scroll_frames": 59,
            "editor_render_ink_pixels": 499,
        }
        self.assertEqual(
            gate.failures(measurements),
            [
                "open_100k_ms",
                "keystroke_to_pixel_p95_ms",
                "scroll_frame_p95_ms",
                "search_1gb_ms",
                "scroll_frames",
                "editor_render_ink_pixels",
            ],
        )


if __name__ == "__main__":
    unittest.main()
