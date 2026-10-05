"""A gaze session the UI can poll while it also sends calibration commands.

One background thread reads the socket. ``latest`` is the newest sample.
``request`` sends one command and waits for its reply. Gaze frames that
arrive during the wait stay in ``latest`` and are not treated as replies.
"""

from __future__ import annotations

import socket
import threading
import time

from tobii_input.client import default_socket_path
from tobii_input.protocol import (
    CMD_ADD_CALIBRATION_POINT,
    CMD_CAL_APPLY,
    CMD_FINISH_CALIBRATION,
    CMD_GET_DISPLAY_AREA,
    CMD_START_CALIBRATION,
    DISPLAY_AREA_SIZE,
    SRV_DISPLAY_AREA,
    SRV_ERR,
    SRV_GAZE,
    SRV_RESPONSE,
    DisplayArea,
    GazeSample,
    TobiiError,
    command_message,
    decode_display_area,
    decode_error,
    decode_gaze,
    display_rect_message,
    subscribe_message,
)

# A sample older than this is treated as "the tracker is quiet". Gaze frames
# arrive many times a second, and calibration pauses them on purpose.
SAMPLE_FRESH_S = 0.5


class GazeSession:
    def __init__(self, path: str | None = None) -> None:
        self.path = path or default_socket_path()
        self._sock: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._latest: GazeSample | None = None
        self._latest_at: float | None = None
        self._pending_cmd: int | None = None
        self._pending = threading.Event()
        self._pending_payload: bytes | None = None
        self._pending_error: TobiiError | None = None
        self._dead: BaseException | None = None

    def connect(self, timeout: float | None = 5.0) -> None:
        self._stop.clear()
        self._open_socket(timeout)
        self._thread = threading.Thread(target=self._read_loop, name="tobii-gaze", daemon=True)
        self._thread.start()

    def _open_socket(self, timeout: float | None) -> None:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        try:
            sock.connect(self.path)
        except OSError:
            sock.close()
            raise
        sock.settimeout(None)
        self._sock = sock
        sock.sendall(subscribe_message())

    def close(self) -> None:
        self._stop.set()
        sock = self._sock
        self._sock = None
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass
        with self._lock:
            if self._pending_cmd is not None and self._pending_error is None:
                self._pending_error = TobiiError("disconnected")
                self._pending.set()

    def __enter__(self) -> GazeSession:
        self.connect()
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def latest(self) -> GazeSample | None:
        with self._lock:
            if self._latest is None or self._latest_at is None:
                return None
            if time.monotonic() - self._latest_at > SAMPLE_FRESH_S:
                return None
            return self._latest

    def request(self, command: int, payload: bytes = b"", timeout: float = 15.0) -> bytes:
        sock = self._sock
        if sock is None:
            raise TobiiError("not connected")
        with self._lock:
            if self._dead is not None:
                raise TobiiError(f"daemon connection closed: {self._dead}")
            self._pending_cmd = command
            self._pending_payload = None
            self._pending_error = None
            self._pending.clear()
        try:
            sock.sendall(command_message(command, payload))
        except OSError as exc:
            with self._lock:
                self._pending_cmd = None
            raise TobiiError(f"send failed: {exc}") from exc
        if not self._pending.wait(timeout):
            with self._lock:
                self._pending_cmd = None
            raise TobiiError(f"no reply to command {command:#x}")
        with self._lock:
            error = self._pending_error
            body = self._pending_payload
            self._pending_cmd = None
        if error is not None:
            raise error
        return body or b""

    def get_display_area(self, timeout: float = 5.0) -> DisplayArea:
        body = self.request(CMD_GET_DISPLAY_AREA, timeout=timeout)
        if len(body) == DISPLAY_AREA_SIZE or len(body) > 2:
            return decode_display_area(body)
        raise TobiiError(f"empty display area reply ({len(body)} bytes)")

    def set_display_rect(
        self,
        width_mm: float,
        height_mm: float,
        origin_x_mm: float,
        origin_y_mm: float,
        z_mm: float = 0.0,
    ) -> None:
        """Tell the tracker where the panel is. This command has no reply."""
        sock = self._sock
        if sock is None:
            raise TobiiError("not connected")
        sock.sendall(display_rect_message(width_mm, height_mm, origin_x_mm, origin_y_mm, z_mm))

    def start_calibration(self) -> None:
        self.request(CMD_START_CALIBRATION, timeout=20.0)

    def add_calibration_point(self, x: float, y: float) -> None:
        import struct

        self.request(CMD_ADD_CALIBRATION_POINT, struct.pack("<2d", x, y), timeout=15.0)

    def finish_calibration(self) -> bytes:
        return self.request(CMD_FINISH_CALIBRATION, timeout=30.0)

    def apply_calibration(self, blob: bytes) -> None:
        self.request(CMD_CAL_APPLY, blob, timeout=60.0)

    def _drop_socket(self) -> None:
        sock = self._sock
        self._sock = None
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass

    def _read_loop(self) -> None:
        while not self._stop.is_set():
            sock = self._sock
            if sock is None:
                if not self._wait_for_daemon():
                    return
                continue
            try:
                while not self._stop.is_set():
                    header = _recv_exact(sock, 5)
                    msg_type = header[0]
                    length = int.from_bytes(header[1:5], "little")
                    if length > 8 * 1024 * 1024:
                        raise TobiiError(f"refusing payload of {length} bytes")
                    payload = _recv_exact(sock, length) if length else b""
                    self._dispatch(msg_type, payload)
            except (OSError, TobiiError, EOFError) as exc:
                with self._lock:
                    self._dead = exc
                    if self._pending_cmd is not None and not self._pending.is_set():
                        self._pending_error = TobiiError(f"daemon connection closed: {exc}")
                        self._pending.set()
                self._drop_socket()

    def _wait_for_daemon(self) -> bool:
        while not self._stop.is_set():
            try:
                self._open_socket(0.5)
            except OSError:
                if self._stop.wait(0.3):
                    return False
                continue
            with self._lock:
                self._dead = None
            return True
        return False

    def _dispatch(self, msg_type: int, payload: bytes) -> None:
        if msg_type == SRV_GAZE:
            sample = decode_gaze(payload)
            with self._lock:
                self._latest = sample
                self._latest_at = time.monotonic()
            return
        if msg_type == SRV_DISPLAY_AREA:
            return
        if msg_type == SRV_RESPONSE:
            if not payload:
                return
            command, body = payload[0], payload[1:]
            with self._lock:
                if self._pending_cmd == command:
                    self._pending_payload = body
                    self._pending.set()
            return
        if msg_type == SRV_ERR:
            error = decode_error(payload)
            with self._lock:
                if self._pending_cmd is not None and (
                    error.command is None or error.command == self._pending_cmd
                ):
                    self._pending_error = error
                    self._pending.set()


def _recv_exact(sock: socket.socket, n: int) -> bytes:
    buf = bytearray()
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise EOFError("daemon closed the socket")
        buf.extend(chunk)
    return bytes(buf)
