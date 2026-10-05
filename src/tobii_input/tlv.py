"""Enough of the Eye Tracker 5 TLV encoding to read a display-area reply.

The daemon forwards the device payload unchanged. After a 2-byte prefix
the body is three point3d values: top-left, top-right, bottom-left.
"""

from __future__ import annotations

import struct

from tobii_input.protocol import DisplayArea, TobiiError, Vec3

_Q42 = float(1 << 42)
_POINT3D = 0x031F41


def _need(buf: bytes, pos: int, n: int) -> None:
    if pos + n > len(buf):
        raise TobiiError(f"display area reply ended at {pos}, needed {n} more bytes")


def _read_point3d(buf: bytes, pos: int) -> tuple[Vec3, int]:
    _need(buf, pos, 9)
    kind, size, tag = struct.unpack_from(">BII", buf, pos)
    pos += 9
    if kind != 5 or size != 4 or tag != _POINT3D:
        raise TobiiError(f"expected a point3d at offset {pos - 9}, got type {kind} tag {tag:#x}")
    coords: list[float] = []
    for _ in range(3):
        _need(buf, pos, 13)
        kind, size, raw = struct.unpack_from(">BIq", buf, pos)
        pos += 13
        if kind != 4 or size != 8:
            raise TobiiError(f"expected a Q42 coordinate, got type {kind} size {size}")
        coords.append(raw / _Q42)
    return Vec3(coords[0], coords[1], coords[2]), pos


def decode_display_area_tlv(payload: bytes) -> DisplayArea:
    """Decode the device reply. The first two bytes are a status prefix."""
    if len(payload) < 2 + 48:
        raise TobiiError(f"display area reply is {len(payload)} bytes")
    pos = 2
    points: list[Vec3] = []
    for _ in range(3):
        point, pos = _read_point3d(payload, pos)
        points.append(point)
    return DisplayArea(top_left=points[0], top_right=points[1], bottom_left=points[2])
