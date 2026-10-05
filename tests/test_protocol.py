import struct
import unittest

from tobii_input.protocol import (
    BIT_FRAME_COUNTER,
    BIT_GAZE_2D,
    BIT_GAZE_3D_L,
    BIT_PUPIL_L,
    BIT_TIMESTAMP,
    BIT_VALIDITY_L,
    GAZE_SIZE,
    HEADER,
    SRV_ERR,
    SRV_GAZE,
    decode_display_area,
    decode_error,
    decode_gaze,
    pop_message,
    subscribe_message,
)


def _gaze(**overrides: float) -> bytes:
    mask = overrides.pop("mask")
    values = {
        "frame": 0,
        "validity_l": 0,
        "validity_r": 4,
        "timestamp": 0,
    }
    doubles = [0.0] * 46
    # pupils, binocular xy at doubles[2], doubles[3]
    doubles[0] = overrides.get("pupil", -1.0)
    doubles[2] = overrides.get("x", 0.0)
    doubles[3] = overrides.get("y", 0.0)
    # gaze_point_left starts at vec index 4 → doubles offset 8 + 12 = 20
    doubles[20] = overrides.get("gx", 0.0)
    doubles[21] = overrides.get("gy", 0.0)
    doubles[22] = overrides.get("gz", 0.0)
    return struct.pack(
        "<4Iq46d",
        mask,
        int(values["frame"] if "frame" not in overrides else overrides["frame"]),
        int(overrides.get("validity_l", 0)),
        int(overrides.get("validity_r", 4)),
        int(overrides.get("timestamp", 0)),
        *doubles,
    )


class ProtocolTest(unittest.TestCase):
    def test_gaze_size_matches_daemon_struct(self) -> None:
        self.assertEqual(GAZE_SIZE, 392)
        self.assertEqual(len(_gaze(mask=0)), 392)

    def test_subscribe_frame(self) -> None:
        frame = subscribe_message()
        self.assertEqual(frame, bytes([0x01, 4, 0, 0, 0, 0x00, 0x05, 0x00, 0x00]))

    def test_present_mask_hides_absent_fields(self) -> None:
        sample = decode_gaze(
            _gaze(mask=BIT_GAZE_2D | BIT_VALIDITY_L | BIT_PUPIL_L, x=0.25, y=0.75, pupil=3.5, validity_l=0)
        )
        self.assertEqual(sample.gaze_xy, (0.25, 0.75))
        self.assertTrue(sample.left_detected)
        self.assertEqual(sample.pupil_left_mm, 3.5)
        self.assertIsNone(sample.timestamp_us)
        self.assertIsNone(sample.frame_counter)
        self.assertIsNone(sample.gaze_point_left_mm)
        self.assertFalse(sample.right_detected)

    def test_three_d_point_offset(self) -> None:
        sample = decode_gaze(_gaze(mask=BIT_GAZE_3D_L | BIT_TIMESTAMP | BIT_FRAME_COUNTER, gx=1.5, gy=-2.0, gz=40.0, timestamp=99, frame=7))
        self.assertEqual(sample.timestamp_us, 99)
        self.assertEqual(sample.frame_counter, 7)
        self.assertIsNotNone(sample.gaze_point_left_mm)
        assert sample.gaze_point_left_mm is not None
        self.assertEqual((sample.gaze_point_left_mm.x, sample.gaze_point_left_mm.y, sample.gaze_point_left_mm.z), (1.5, -2.0, 40.0))

    def test_framing_across_chunks(self) -> None:
        payload = _gaze(mask=BIT_GAZE_2D, x=0.5, y=0.5)
        raw = HEADER.pack(SRV_GAZE, len(payload)) + payload
        buf = bytearray(raw[:3])
        self.assertIsNone(pop_message(buf))
        buf.extend(raw[3:])
        got = pop_message(buf)
        self.assertIsNotNone(got)
        assert got is not None
        self.assertEqual(got[0], SRV_GAZE)
        self.assertEqual(decode_gaze(got[1]).gaze_xy, (0.5, 0.5))
        self.assertEqual(buf, bytearray())

    def test_display_area_and_error(self) -> None:
        area = decode_display_area(struct.pack("<9d", 0, 10, 5, 300, 10, 5, 0, -10, 5))
        self.assertEqual(area.top_right.x, 300)
        err = decode_error(bytes([0x02]) + struct.pack("<I", 1))
        self.assertEqual(err.command, 0x02)
        self.assertEqual(err.code, 1)


if __name__ == "__main__":
    unittest.main()
