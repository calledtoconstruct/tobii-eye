"""Map this machine's panel into the tracker's millimeter frame.

The tracker origin is the IR array. X is right, Y is up, Z is toward the
person. The Eye Tracker 5 sits under the panel, so the panel's bottom edge
is a few millimeters above the origin and the panel is centered on X.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass

from tobii_input.protocol import DisplayArea

# Distance from the sensor array up to the bottom edge of the panel.
BELOW_PANEL_MM = 10.0
CONFIG_PATH = os.path.expanduser("~/.config/tobii.json")


@dataclass(frozen=True)
class Panel:
    width_mm: float
    height_mm: float
    origin_x_mm: float
    origin_y_mm: float
    z_mm: float = 0.0

    @property
    def plausible(self) -> bool:
        return 80.0 <= self.width_mm <= 800.0 and 50.0 <= self.height_mm <= 500.0


def panel_from_monitor(width_mm: float, height_mm: float, below_mm: float = BELOW_PANEL_MM) -> Panel:
    return Panel(
        width_mm=width_mm,
        height_mm=height_mm,
        origin_x_mm=-width_mm / 2.0,
        origin_y_mm=below_mm,
    )


def plane_size(area: DisplayArea) -> tuple[float, float]:
    width = abs(area.top_right.x - area.top_left.x)
    height = abs(area.top_left.y - area.bottom_left.y)
    return width, height


def focused_monitor_mm() -> tuple[float, float] | None:
    """Physical size of the focused Hyprland monitor, when hyprctl can say."""
    try:
        raw = subprocess.check_output(["hyprctl", "monitors", "-j"], text=True, timeout=2)
        monitors = json.loads(raw)
    except (OSError, subprocess.SubprocessError, json.JSONDecodeError):
        return None
    chosen = next((m for m in monitors if m.get("focused")), None)
    if chosen is None and monitors:
        chosen = monitors[0]
    if not chosen:
        return None
    width = chosen.get("physicalWidth")
    height = chosen.get("physicalHeight")
    if not width or not height:
        return None
    return float(width), float(height)


def align_panel(session, path: str = CONFIG_PATH) -> str:
    """Point the tracker at this panel when its plane is still the reset default.

    ``session`` is a ``GazeSession``. Imported lazily so this module stays free of it.
    """
    from tobii_input.protocol import TobiiError

    try:
        area = session.get_display_area()
    except TobiiError as exc:
        return f"Could not read the display plane ({exc})."
    width, height = plane_size(area)
    measured = focused_monitor_mm()
    if measured is None:
        return f"Display plane is {width:.0f} by {height:.0f} mm."
    panel = panel_from_monitor(*measured)
    # 1500x1000 is the daemon's built-in placeholder, not this panel.
    if panel.plausible and (width > 800 or height > 500 or width < 80 or height < 50):
        session.set_display_rect(
            panel.width_mm, panel.height_mm, panel.origin_x_mm, panel.origin_y_mm, panel.z_mm
        )
        remember_panel(panel, path)
        return (
            f"Set the display plane to {panel.width_mm:.0f} by {panel.height_mm:.0f} mm, "
            f"centered, {panel.origin_y_mm:.0f} mm above the sensor."
        )
    return f"Display plane is {width:.0f} by {height:.0f} mm."


def remember_panel(panel: Panel, path: str = CONFIG_PATH) -> None:
    """Write the daemon config used the next time the tracker resets its plane."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    document = {
        "display_area": {
            "w_mm": panel.width_mm,
            "h_mm": panel.height_mm,
            "z_mm": panel.z_mm,
            "tilt": 0,
            "cx": 0,
            "cy": f"b - {BELOW_PANEL_MM:g}",
        }
    }
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(document, fh, indent=2)
        fh.write("\n")
