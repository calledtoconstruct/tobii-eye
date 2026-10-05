import os
import socket
import struct
import tempfile
import threading
import time
import unittest

from tobii_input.live import SAMPLE_FRESH_S, GazeSession
from tobii_input.protocol import decode_gaze
from tobii_input.panel import panel_from_monitor, plane_size
from tobii_input.protocol import (
    BIT_GAZE_2D,
    BIT_VALIDITY_L,
    CMD_ADD_CALIBRATION_POINT,
    CMD_GET_DISPLAY_AREA,
    GAZE_SIZE,
    HEADER,
    SRV_GAZE,
    SRV_RESPONSE,
    Vec3,
    decode_display_area,
)
from tobii_input.tlv import decode_display_area_tlv


def _q42(value: float) -> bytes:
    raw = int(round(value * float(1 << 42)))
    return struct.pack(">BIq", 4, 8, raw)


def _point(x: float, y: float, z: float) -> bytes:
    return struct.pack(">BII", 5, 4, 0x031F41) + _q42(x) + _q42(y) + _q42(z)


def _gaze(x: float, y: float) -> bytes:
    doubles = [0.0] * 46
    doubles[2] = x
    doubles[3] = y
    return struct.pack("<4Iq46d", BIT_GAZE_2D | BIT_VALIDITY_L, 3, 0, 4, 10, *doubles)


class TlvTest(unittest.TestCase):
    def test_display_area_tlv(self) -> None:
        payload = b"\x00\x00" + _point(-145, 180, 0) + _point(145, 180, 1) + _point(-145, 10, 0)
        area = decode_display_area_tlv(payload)
        self.assertAlmostEqual(area.top_left.x, -145, places=3)
        self.assertAlmostEqual(area.top_right.y, 180, places=3)
        self.assertAlmostEqual(area.bottom_left.y, 10, places=3)
        self.assertAlmostEqual(plane_size(area)[0], 290, places=3)
        again = decode_display_area(payload)
        self.assertEqual(again.top_left, Vec3(area.top_left.x, area.top_left.y, area.top_left.z))

    def test_panel_is_centered_above_the_sensor(self) -> None:
        panel = panel_from_monitor(290, 170)
        self.assertEqual(panel.origin_x_mm, -145)
        self.assertEqual(panel.origin_y_mm, 10)
        self.assertTrue(panel.plausible)


class SessionTest(unittest.TestCase):
    def test_gaze_and_command_share_the_socket(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = os.path.join(directory.name, "gaze.sock")
        ready = threading.Event()

        def serve() -> None:
            listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            listener.bind(path)
            listener.listen(1)
            ready.set()
            conn, _ = listener.accept()
            with conn:
                self.assertEqual(conn.recv(9)[:1], b"\x01")
                frame = _gaze(0.2, 0.8)
                self.assertEqual(len(frame), GAZE_SIZE)
                conn.sendall(HEADER.pack(SRV_GAZE, len(frame)) + frame)
                header = conn.recv(5)
                msg_type, length = HEADER.unpack(header)
                self.assertEqual(msg_type, CMD_GET_DISPLAY_AREA)
                body = conn.recv(length) if length else b""
                self.assertEqual(body, b"")
                reply = b"\x00\x00" + _point(0, 10, 0) + _point(100, 10, 0) + _point(0, 0, 0)
                conn.sendall(HEADER.pack(SRV_RESPONSE, 1 + len(reply)) + bytes([CMD_GET_DISPLAY_AREA]) + reply)
                header = conn.recv(5)
                msg_type, length = HEADER.unpack(header)
                self.assertEqual(msg_type, CMD_ADD_CALIBRATION_POINT)
                point = conn.recv(length)
                self.assertEqual(struct.unpack("<2d", point), (0.5, 0.5))
                conn.sendall(HEADER.pack(SRV_RESPONSE, 1) + bytes([CMD_ADD_CALIBRATION_POINT]))
            listener.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        self.assertTrue(ready.wait(2))
        with GazeSession(path) as session:
            import time
            deadline = time.monotonic() + 2
            sample = None
            while sample is None and time.monotonic() < deadline:
                sample = session.latest()
                time.sleep(0.01)
            self.assertIsNotNone(sample)
            assert sample is not None
            self.assertEqual(sample.gaze_xy, (0.2, 0.8))
            self.assertTrue(sample.left_detected)
            area = session.get_display_area()
            self.assertAlmostEqual(area.top_right.x, 100, places=3)
            session.add_calibration_point(0.5, 0.5)
        thread.join(2)

    def test_a_quiet_stream_is_not_a_fresh_gaze(self) -> None:
        session = GazeSession("/no/such/socket")
        session._latest = decode_gaze(_gaze(0.1, 0.2))
        session._latest_at = time.monotonic()
        self.assertEqual(session.latest().gaze_xy, (0.1, 0.2))
        session._latest_at -= SAMPLE_FRESH_S + 0.1
        self.assertIsNone(session.latest())

    def test_session_follows_a_daemon_restart(self) -> None:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = os.path.join(directory.name, "gaze.sock")
        ready = threading.Event()
        seen = threading.Event()

        def serve() -> None:
            listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            listener.bind(path)
            listener.listen(1)
            ready.set()
            conn, _ = listener.accept()
            conn.recv(9)
            frame = _gaze(0.2, 0.5)
            conn.sendall(HEADER.pack(SRV_GAZE, len(frame)) + frame)
            self.assertTrue(seen.wait(2))
            conn.close()
            conn, _ = listener.accept()
            conn.recv(9)
            frame = _gaze(0.9, 0.5)
            conn.sendall(HEADER.pack(SRV_GAZE, len(frame)) + frame)
            conn.close()
            listener.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        self.assertTrue(ready.wait(2))
        session = GazeSession(path)
        self.addCleanup(session.close)
        session.connect()

        def wait_for(x: float) -> None:
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                sample = session.latest()
                if sample is not None and sample.gaze_xy == (x, 0.5):
                    return
                time.sleep(0.01)
            self.fail(f"gaze did not become {x}")

        wait_for(0.2)
        seen.set()
        wait_for(0.9)
        thread.join(2)
