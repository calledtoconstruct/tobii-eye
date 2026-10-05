"""Print live gaze from the local daemon.

    python -m tobii_input --count 30
    python -m tobii_input --json
"""

from __future__ import annotations

import argparse
import json
import sys

from tobii_input.client import TobiiClient, default_socket_path
from tobii_input.protocol import GazeSample, Vec3


def _vec(value: Vec3 | None) -> list[float] | None:
    if value is None:
        return None
    return [value.x, value.y, value.z]


def sample_dict(sample: GazeSample) -> dict:
    return {
        "frame": sample.frame_counter,
        "timestamp_us": sample.timestamp_us,
        "validity_left": sample.validity_left,
        "validity_right": sample.validity_right,
        "pupil_left_mm": sample.pupil_left_mm,
        "pupil_right_mm": sample.pupil_right_mm,
        "gaze_xy": None if sample.gaze_xy is None else list(sample.gaze_xy),
        "gaze_xy_left": None if sample.gaze_xy_left is None else list(sample.gaze_xy_left),
        "gaze_xy_right": None if sample.gaze_xy_right is None else list(sample.gaze_xy_right),
        "eye_origin_left_mm": _vec(sample.eye_origin_left_mm),
        "eye_origin_right_mm": _vec(sample.eye_origin_right_mm),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Read Tobii Eye Tracker 5 gaze from tobiifreed")
    parser.add_argument("--socket", default=default_socket_path(), help="Unix socket path")
    parser.add_argument("--count", type=int, default=0, help="Stop after this many samples (0 = until interrupted)")
    parser.add_argument("--json", action="store_true", help="Print one JSON object per sample")
    parser.add_argument("--display-area", action="store_true", help="Print the display plane and exit")
    args = parser.parse_args(argv)

    client = TobiiClient(args.socket)
    try:
        client.connect()
    except OSError as exc:
        print(f"cannot connect to {args.socket}: {exc}", file=sys.stderr)
        print("start prefix/bin/tobiifreed after the udev rule is installed", file=sys.stderr)
        return 1

    seen = 0
    try:
        if args.display_area:
            area = client.get_display_area()
            print(
                f"top_left={area.top_left} top_right={area.top_right} bottom_left={area.bottom_left}"
            )
            return 0
        for sample in client.samples():
            if args.json:
                print(json.dumps(sample_dict(sample)), flush=True)
            else:
                xy = sample.gaze_xy
                point = "none" if xy is None else f"{xy[0]:.3f},{xy[1]:.3f}"
                print(
                    f"frame={sample.frame_counter} "
                    f"eyes={int(sample.left_detected)}{int(sample.right_detected)} "
                    f"gaze={point} "
                    f"pupil={sample.pupil_left_mm},{sample.pupil_right_mm}",
                    flush=True,
                )
            seen += 1
            if args.count and seen >= args.count:
                break
    except KeyboardInterrupt:
        return 0
    finally:
        client.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
