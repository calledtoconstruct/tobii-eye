import os
import socket
import struct
import tempfile
import threading
import unittest

from tobii_input.client import TobiiClient
from tobii_input.protocol import BIT_GAZE_2D, GAZE_SIZE, HEADER, SRV_GAZE


def _payload(x: float, y: float) -> bytes:
    doubles = [0.0] * 46
    doubles[2] = x
    doubles[3] = y
    return struct.pack("<4Iq46d", BIT_GAZE_2D, 1, 0, 0, 0, *doubles)


class ClientTest(unittest.TestCase):
    def test_round_trip_against_a_fake_daemon(self) -> None:
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
                header = conn.recv(9)
                self.assertEqual(header[:1], b"\x01")
                frame = _payload(0.4, 0.6)
                self.assertEqual(len(frame), GAZE_SIZE)
                conn.sendall(HEADER.pack(SRV_GAZE, len(frame)) + frame)
            listener.close()

        thread = threading.Thread(target=serve)
        thread.start()
        self.assertTrue(ready.wait(2))
        with TobiiClient(path) as client:
            sample = next(client.samples())
        thread.join(2)
        self.assertEqual(sample.gaze_xy, (0.4, 0.6))
        self.assertIsNone(sample.pupil_left_mm)
