"""Read gaze from the current device adapter through the local tobiifreed socket."""

from tobii_input.client import TobiiClient, default_socket_path
from tobii_input.protocol import (
    DisplayArea,
    GazeSample,
    TobiiError,
    decode_gaze,
)

__all__ = [
    "DisplayArea",
    "GazeSample",
    "TobiiClient",
    "TobiiError",
    "decode_gaze",
    "default_socket_path",
]
