import unittest

from tobii_input.warp import Fixation, apply_homography, fit_homography, mean_miss


class WarpTest(unittest.TestCase):
    def test_shift_is_recovered(self) -> None:
        sources = [(0.12, 0.12), (0.88, 0.12), (0.12, 0.88), (0.88, 0.88), (0.5, 0.5)]
        pairs = [((x, y), (x + 0.1, y - 0.05)) for x, y in sources]
        homography = fit_homography(pairs)
        self.assertIsNotNone(homography)
        assert homography is not None
        moved = apply_homography(homography, (0.4, 0.6))
        self.assertAlmostEqual(moved[0], 0.5, places=4)
        self.assertAlmostEqual(moved[1], 0.55, places=4)
        self.assertLess(mean_miss(homography, pairs), 1e-6)

    def test_fixation_waits_for_a_new_still_gaze(self) -> None:
        fix = Fixation()
        fix.anchor = (0.5, 0.5)
        self.assertEqual(fix.update((0.52, 0.48), 0.0), 0.0)
        self.assertEqual(fix.update((0.52, 0.49), 1.0), 0.0)
        self.assertEqual(fix.update((0.15, 0.15), 2.0), 0.0)
        self.assertGreater(fix.update((0.15, 0.16), 2.0 + Fixation.HOLD), 0.99)
        self.assertIsNotNone(fix.median())
