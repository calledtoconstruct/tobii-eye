"""Binary framing for the tobiifreed Unix socket.

The layout matches Aetherall/tobiifree commit ad82906
(driver/src/daemon_protocol.zig and the GazeSample extern struct).
A message is ``[u8 type][u32 little-endian length][payload]``.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass

HEADER = struct.Struct("<BI")
HEADER_SIZE = HEADER.size  # 5

CMD_SUBSCRIBE = 0x01
CMD_GET_DISPLAY_AREA = 0x02
CMD_SET_DISPLAY_AREA = 0x03
CMD_START_CALIBRATION = 0x20
CMD_ADD_CALIBRATION_POINT = 0x21
CMD_FINISH_CALIBRATION = 0x22
CMD_CAL_APPLY = 0x23
CMD_DISCONNECT = 0xFF

SRV_GAZE = 0x01
SRV_RESPONSE = 0x02
SRV_DISPLAY_AREA = 0x03
SRV_ERR = 0xFF

STREAM_GAZE = 0x500

# present_mask bits. Absent fields stay None.
BIT_TIMESTAMP = 1 << 0
BIT_FRAME_COUNTER = 1 << 1
BIT_VALIDITY_L = 1 << 2
BIT_VALIDITY_R = 1 << 3
BIT_PUPIL_L = 1 << 4
BIT_PUPIL_R = 1 << 5
BIT_GAZE_2D = 1 << 6
BIT_GAZE_2D_L = 1 << 7
BIT_GAZE_2D_R = 1 << 8
BIT_EYE_ORIGIN_L = 1 << 9
BIT_EYE_ORIGIN_R = 1 << 10
BIT_TRACKBOX_L = 1 << 11
BIT_TRACKBOX_R = 1 << 12
BIT_GAZE_3D_L = 1 << 13
BIT_GAZE_3D_R = 1 << 14
BIT_EYE_ORIGIN_L_DISP = 1 << 15
BIT_EYE_ORIGIN_R_DISP = 1 << 16
BIT_TRACKBOX_L_DISP = 1 << 17
BIT_TRACKBOX_R_DISP = 1 << 18
BIT_EYE_ORIGIN_RAW_L = 1 << 19
BIT_EYE_ORIGIN_RAW_R = 1 << 20
BIT_GAZE_2D_UNFILTERED = 1 << 21

# 4xu32 + i64 + 46xf64 = 392 bytes.
_GAZE = struct.Struct("<4Iq46d")
GAZE_SIZE = _GAZE.size

# Nine little-endian f64: top-left, top-right, bottom-left, each xyz mm.
_AREA = struct.Struct("<9d")
DISPLAY_AREA_SIZE = _AREA.size

# 0 = the tracker sees the eye. 4 = not detected.
VALID = 0


class TobiiError(Exception):
    """The daemon reported a failed command, or a frame was the wrong size."""

    def __init__(self, message: str, code: int | None = None, command: int | None = None):
        super().__init__(message)
        self.code = code
        self.command = command


@dataclass(frozen=True)
class Vec3:
    x: float
    y: float
    z: float


@dataclass(frozen=True)
class GazeSample:
    """One gaze frame. Fields the tracker omitted are None.

    ``gaze_xy`` is the filtered binocular point on the configured display
    area, in normalized coordinates. ``(0, 0)`` is the top-left of that
    area and ``(1, 1)`` is the bottom-right. Validity ``0`` means the eye
    was detected; ``4`` means it was not.
    """

    present_mask: int
    frame_counter: int | None
    timestamp_us: int | None
    validity_left: int | None
    validity_right: int | None
    pupil_left_mm: float | None
    pupil_right_mm: float | None
    gaze_xy: tuple[float, float] | None
    gaze_xy_left: tuple[float, float] | None
    gaze_xy_right: tuple[float, float] | None
    gaze_xy_unfiltered: tuple[float, float] | None
    eye_origin_left_mm: Vec3 | None
    eye_origin_right_mm: Vec3 | None
    gaze_point_left_mm: Vec3 | None
    gaze_point_right_mm: Vec3 | None

    @property
    def left_detected(self) -> bool:
        return self.validity_left == VALID

    @property
    def right_detected(self) -> bool:
        return self.validity_right == VALID


@dataclass(frozen=True)
class DisplayArea:
    """Display plane corners in tracker millimeters.

    Origin is the IR sensor array. X is right, Y is up, Z is toward the user.
    """

    top_left: Vec3
    top_right: Vec3
    bottom_left: Vec3


def subscribe_message() -> bytes:
    """Ask the daemon to start forwarding gaze stream 0x500."""
    return HEADER.pack(CMD_SUBSCRIBE, 4) + struct.pack("<I", STREAM_GAZE)


def command_message(command: int, payload: bytes = b"") -> bytes:
    return HEADER.pack(command, len(payload)) + payload


def pop_message(buf: bytearray) -> tuple[int, bytes] | None:
    """Remove one complete frame from the front of ``buf``, if one is there."""
    if len(buf) < HEADER_SIZE:
        return None
    msg_type, length = HEADER.unpack_from(buf)
    end = HEADER_SIZE + length
    if len(buf) < end:
        return None
    if length > 8 * 1024 * 1024:
        raise TobiiError(f"refusing payload of {length} bytes")
    payload = bytes(buf[HEADER_SIZE:end])
    del buf[:end]
    return msg_type, payload


def _vec3(values: tuple[float, ...], index: int) -> Vec3:
    base = index * 3
    return Vec3(values[base], values[base + 1], values[base + 2])


def _xy(values: tuple[float, ...], index: int) -> tuple[float, float]:
    base = index * 2
    return values[base], values[base + 1]


def decode_gaze(payload: bytes) -> GazeSample:
    if len(payload) != GAZE_SIZE:
        raise TobiiError(f"gaze payload is {len(payload)} bytes, expected {GAZE_SIZE}")
    raw = _GAZE.unpack(payload)
    mask = raw[0]
    frame = raw[1]
    validity_l = raw[2]
    validity_r = raw[3]
    timestamp = raw[4]
    doubles = raw[5:]

    def bit(flag: int, value):
        return value if mask & flag else None

    # doubles: pupils[2], gaze2d x3 [6], twelve Vec3 [36], unfiltered [2]
    pupils = doubles[0:2]
    gaze2d = doubles[2:8]
    vecs = doubles[8:44]
    unfiltered = doubles[44:46]
    return GazeSample(
        present_mask=mask,
        frame_counter=bit(BIT_FRAME_COUNTER, frame),
        timestamp_us=bit(BIT_TIMESTAMP, timestamp),
        validity_left=bit(BIT_VALIDITY_L, validity_l),
        validity_right=bit(BIT_VALIDITY_R, validity_r),
        pupil_left_mm=bit(BIT_PUPIL_L, pupils[0]),
        pupil_right_mm=bit(BIT_PUPIL_R, pupils[1]),
        gaze_xy=bit(BIT_GAZE_2D, _xy(gaze2d, 0)),
        gaze_xy_left=bit(BIT_GAZE_2D_L, _xy(gaze2d, 1)),
        gaze_xy_right=bit(BIT_GAZE_2D_R, _xy(gaze2d, 2)),
        gaze_xy_unfiltered=bit(BIT_GAZE_2D_UNFILTERED, (unfiltered[0], unfiltered[1])),
        eye_origin_left_mm=bit(BIT_EYE_ORIGIN_L, _vec3(vecs, 0)),
        eye_origin_right_mm=bit(BIT_EYE_ORIGIN_R, _vec3(vecs, 1)),
        # vecs[2] and vecs[3] are normalized track-box positions, not a direction.
        gaze_point_left_mm=bit(BIT_GAZE_3D_L, _vec3(vecs, 4)),
        gaze_point_right_mm=bit(BIT_GAZE_3D_R, _vec3(vecs, 5)),
    )


def decode_display_area(payload: bytes) -> DisplayArea:
    """Accept either 9 raw little-endian doubles or the device TLV reply."""
    if len(payload) == DISPLAY_AREA_SIZE:
        v = _AREA.unpack(payload)
        return DisplayArea(
            top_left=Vec3(v[0], v[1], v[2]),
            top_right=Vec3(v[3], v[4], v[5]),
            bottom_left=Vec3(v[6], v[7], v[8]),
        )
    from tobii_input.tlv import decode_display_area_tlv

    return decode_display_area_tlv(payload)


def display_rect_message(width_mm: float, height_mm: float, origin_x_mm: float, origin_y_mm: float, z_mm: float) -> bytes:
    """Set the display plane. The daemon does not reply to this command."""
    payload = struct.pack("<5d", width_mm, height_mm, origin_x_mm, origin_y_mm, z_mm)
    return command_message(CMD_SET_DISPLAY_AREA, payload)


def calibration_point_message(x: float, y: float) -> bytes:
    return command_message(CMD_ADD_CALIBRATION_POINT, struct.pack("<2d", x, y))


def decode_error(payload: bytes) -> TobiiError:
    if len(payload) != 5:
        return TobiiError(f"short error payload ({len(payload)} bytes)")
    command = payload[0]
    code = struct.unpack_from("<I", payload, 1)[0]
    return TobiiError(f"daemon error {code} for command {command:#x}", code=code, command=command)
