"""Connect to tobiifreed and iterate gaze samples.

The daemon owns the USB device. This process only speaks the Unix socket
at ``$XDG_RUNTIME_DIR/tobiifreed/gaze.sock``.
"""

from __future__ import annotations

import os
import socket
from collections.abc import Iterator

from tobii_input.protocol import (
    CMD_DISCONNECT,
    CMD_GET_DISPLAY_AREA,
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
    pop_message,
    subscribe_message,
)


def default_socket_path() -> str:
    runtime = os.environ.get("XDG_RUNTIME_DIR", "/tmp")
    return os.path.join(runtime, "tobiifreed", "gaze.sock")


class TobiiClient:
    """One connection to a running tobiifreed.

    Call :meth:`connect` before reading. Gaze is pushed continuously after
    subscribe; other messages are skipped by :meth:`samples` and returned
    by :meth:`events`.
    """

    def __init__(self, path: str | None = None) -> None:
        self.path = path or default_socket_path()
        self._sock: socket.socket | None = None
        self._buf = bytearray()

    def connect(self, timeout: float | None = 5.0) -> None:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(timeout)
        try:
            sock.connect(self.path)
        except OSError:
            sock.close()
            raise
        # connect() used a deadline. Gaze reads block until the daemon
        # sends a frame or closes the socket.
        sock.settimeout(None)
        self._sock = sock
        sock.sendall(subscribe_message())

    def close(self) -> None:
        sock = self._sock
        self._sock = None
        if sock is None:
            return
        try:
            sock.sendall(command_message(CMD_DISCONNECT))
        except OSError:
            pass
        sock.close()

    def __enter__(self) -> TobiiClient:
        self.connect()
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def events(self) -> Iterator[GazeSample | DisplayArea | tuple[int, bytes]]:
        """Yield gaze samples, display-area updates, and ``(command, payload)`` replies."""
        while True:
            message = self._next_message()
            if message is None:
                return
            msg_type, payload = message
            if msg_type == SRV_GAZE:
                yield decode_gaze(payload)
            elif msg_type == SRV_DISPLAY_AREA:
                yield decode_display_area(payload)
            elif msg_type == SRV_RESPONSE:
                if not payload:
                    raise TobiiError("empty command response")
                yield payload[0], payload[1:]
            elif msg_type == SRV_ERR:
                raise decode_error(payload)
            else:
                raise TobiiError(f"unknown daemon message {msg_type:#x}")

    def samples(self) -> Iterator[GazeSample]:
        for event in self.events():
            if isinstance(event, GazeSample):
                yield event

    def get_display_area(self) -> DisplayArea:
        """Request the tracker's display plane. Gaze frames that arrive first are discarded."""
        self._require().sendall(command_message(CMD_GET_DISPLAY_AREA))
        for event in self.events():
            if isinstance(event, DisplayArea):
                return event
            if isinstance(event, tuple) and event[0] == CMD_GET_DISPLAY_AREA:
                return decode_display_area(event[1])
        raise TobiiError("daemon closed before sending the display area")

    def _require(self) -> socket.socket:
        if self._sock is None:
            raise TobiiError("not connected")
        return self._sock

    def _next_message(self) -> tuple[int, bytes] | None:
        sock = self._require()
        while True:
            got = pop_message(self._buf)
            if got is not None:
                return got
            try:
                chunk = sock.recv(65536)
            except socket.timeout:
                raise
            if not chunk:
                return None
            self._buf.extend(chunk)
