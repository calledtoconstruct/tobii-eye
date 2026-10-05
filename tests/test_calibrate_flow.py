import unittest
from unittest.mock import patch

from tobii_input.desktop import POINTS, CalibrateStage, ScreenCorrection
from tobii_input.warp import Fixation


class _Session:
    def __init__(self) -> None:
        self.points: list[tuple[float, float]] = []
        self.finished = False
        self.applied = None
        self.sample = None

    def latest(self):
        return self.sample

    def start_calibration(self) -> None:
        return None

    def add_calibration_point(self, x: float, y: float) -> None:
        self.points.append((x, y))

    def finish_calibration(self) -> bytes:
        self.finished = True
        return b"blob"

    def apply_calibration(self, blob: bytes) -> None:
        self.applied = blob


class CalibrateFlowTest(unittest.TestCase):
    def test_device_pass_updates_the_tracker_before_the_screen_check(self) -> None:
        session = _Session()
        identity = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
        stage = CalibrateStage(session, ScreenCorrection(identity), lambda: None)
        stage._begin()
        self.assertEqual(stage.phase, "device")

        for target in POINTS:
            stage.fixation.samples = [target]
            stage._capture()

        self.assertEqual(session.points, list(POINTS))
        self.assertTrue(session.finished)
        self.assertEqual(session.applied, b"blob")
        self.assertEqual(stage.checks, [])
        self.assertEqual(stage.phase, "check")
        self.assertEqual(stage.index, 0)
        self.assertEqual(stage.fixation.anchor, POINTS[-1])

        with patch("tobii_input.desktop.save_warp") as save:
            for target in POINTS:
                measured = (target[0] + 0.1, target[1] - 0.05)
                stage.fixation.samples = [measured]
                stage._capture()
            save.assert_called_once()

        self.assertEqual(stage.phase, "done")
        self.assertEqual([pair[1] for pair in stage.checks], list(POINTS))
        self.assertEqual(stage.checks[0][0], (POINTS[0][0] + 0.1, POINTS[0][1] - 0.05))
        self.assertIsNotNone(stage.correction.homography)

    def test_missing_gaze_does_not_advance(self) -> None:
        session = _Session()
        identity = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
        stage = CalibrateStage(session, ScreenCorrection(identity), lambda: None)
        stage.phase = "device"
        stage.fixation = Fixation()
        stage._capture()
        self.assertEqual(stage.index, 0)
        self.assertEqual(session.points, [])
        self.assertEqual(stage.phase, "device")
