"""Fullscreen calibration and a short gaze game.

Calibration records five places the person is looking. Pop scores how long
the gaze stays inside a target. Both need ``tobiifreed`` on its socket.
Drawing is GTK 4, which is what this Wayland session already has. The Tk
module is installed without ``libtk``.
"""

from __future__ import annotations

import math
import random
import time

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, GLib, Gtk

from tobii_input.live import GazeSession
from tobii_input.panel import align_panel
from tobii_input.protocol import GazeSample, TobiiError
from tobii_input.warp import (
    Fixation,
    Matrix,
    apply_homography,
    fit_homography,
    load_warp,
    mean_miss,
    save_warp,
)

POINTS = (
    (0.5, 0.5),
    (0.12, 0.12),
    (0.88, 0.12),
    (0.12, 0.88),
    (0.88, 0.88),
)

BG = (0.063, 0.078, 0.059)
INK = (0.957, 0.945, 0.910)
DIM = (0.553, 0.573, 0.518)
GOOD = (0.843, 1.0, 0.290)
GAZE = (0.494, 0.878, 1.0)
WARN = (1.0, 0.690, 0.529)
TARGET = (1.0, 0.820, 0.400)


def eyes_seen(sample: GazeSample | None) -> bool:
    if sample is None:
        return False
    return sample.left_detected or sample.right_detected


def gaze_norm(sample: GazeSample | None) -> tuple[float, float] | None:
    if sample is None or sample.gaze_xy is None or not eyes_seen(sample):
        return None
    return sample.gaze_xy


class ScreenCorrection:
    """The homography Pop and the finished calibration cursor both use."""

    def __init__(self, homography: Matrix | None = None) -> None:
        self.homography = load_warp() if homography is None else homography

    def point(self, sample: GazeSample | None) -> tuple[float, float] | None:
        raw = gaze_norm(sample)
        if raw is None or self.homography is None:
            return raw
        return apply_homography(self.homography, raw)


def _rgb(cr, color: tuple[float, float, float]) -> None:
    cr.set_source_rgb(*color)


def _text(cr, x: float, y: float, value: str, size: int, color: tuple[float, float, float], anchor: str = "center") -> None:
    _rgb(cr, color)
    cr.select_font_face("sans-serif")
    cr.set_font_size(size)
    _bearing_x, _bearing_y, text_width, text_height, _advance_x, _advance_y = cr.text_extents(value)
    if anchor == "center":
        tx = x - text_width / 2
    elif anchor == "e":
        tx = x - text_width
    else:
        tx = x
    cr.move_to(tx, y + text_height / 2)
    cr.show_text(value)


class CalibrateStage:
    def __init__(self, session: GazeSession, correction: ScreenCorrection, on_play) -> None:
        self.session = session
        self.correction = correction
        self.on_play = on_play
        self.index = 0
        self.phase = "intro"  # intro, device, check, send, done, error
        self.fixation = Fixation()
        self.capture_queued = False
        self.checks: list[tuple[tuple[float, float], tuple[float, float]]] = []
        self.miss = None
        self.message = "Space starts. Look at each dot and hold still. Esc quits."
        self.error = ""

    def key(self, keyval: int) -> None:
        if keyval in (Gdk.KEY_space, Gdk.KEY_Return):
            self._space()
        elif keyval in (Gdk.KEY_p, Gdk.KEY_P) and self.phase == "done":
            self.on_play()

    def _space(self) -> None:
        if self.phase == "intro":
            self._begin()
        elif self.phase in {"device", "check"} and eyes_seen(self.session.latest()):
            self._capture()
        elif self.phase == "done":
            self.on_play()

    def _begin(self) -> None:
        self.phase = "send"
        self.message = "Starting calibration."
        try:
            self.session.start_calibration()
        except TobiiError as exc:
            self.phase = "error"
            self.error = str(exc)
            return
        self.index = 0
        self.fixation = Fixation()
        self.capture_queued = False
        self.phase = "device"
        self.message = "Look at the dot until the ring closes. Space records it now."

    def _capture(self) -> None:
        self.capture_queued = False
        if self.phase not in {"device", "check"} or self.index >= len(POINTS):
            return
        recording = self.phase
        target = POINTS[self.index]
        measured = self.fixation.median() or gaze_norm(self.session.latest())
        if measured is None:
            self.message = "The tracker does not see your eyes."
            return
        self.phase = "send"
        self.message = f"Recording point {self.index + 1}."
        try:
            if recording == "device":
                # The tracker samples the eyes itself. Send the dot.
                self.session.add_calibration_point(*target)
            else:
                self.checks.append((measured, target))
        except TobiiError as exc:
            self.phase = "error"
            self.error = str(exc)
            return
        self.fixation.anchor = measured
        self.fixation.reset_point()
        self.index += 1
        if self.index < len(POINTS):
            self.phase = recording
            self.message = "Look at the next dot. The ring waits until your gaze moves, then holds still."
            return
        if recording == "device":
            self._finish_device()
        else:
            self._finish_check()

    def _finish_device(self) -> None:
        self.message = "Saving the tracker calibration."
        try:
            blob = self.session.finish_calibration()
            self.session.apply_calibration(blob)
        except TobiiError as exc:
            self.phase = "error"
            self.error = str(exc)
            return
        self.index = 0
        # Keep the last accepted gaze so the center check waits for a real look.
        anchor = self.fixation.anchor
        self.fixation = Fixation()
        self.fixation.anchor = anchor
        self.phase = "check"
        self.message = "Tracker updated. Look at the same dots again so the cursor can be corrected."

    def _finish_check(self) -> None:
        homography = fit_homography(self.checks)
        if homography is None:
            self.phase = "error"
            self.error = "Could not fit the screen correction from those dots."
            return
        self.correction.homography = homography
        save_warp(homography)
        self.miss = mean_miss(homography, self.checks)
        self.phase = "done"
        self.message = "Screen correction saved. Space or P plays Pop. Esc quits."

    def draw(self, cr, width: int, height: int) -> None:
        sample = self.session.latest()
        seen = eyes_seen(sample)
        _text(cr, 24, 28, "Calibrate", 18, DIM, "w")
        _text(cr, width - 24, 28, "eyes visible" if seen else "no eyes", 16, GOOD if seen else WARN, "e")
        if self.phase == "error":
            _text(cr, width / 2, height / 2, self.error, 18, WARN)
            _text(cr, width / 2, height / 2 + 40, "Esc quits.", 16, DIM)
        elif self.phase in {"device", "check", "send"} and self.index < len(POINTS):
            nx, ny = POINTS[self.index]
            label = f"{self.index + 1} / {len(POINTS)}"
            if self.phase == "check":
                label = f"check {label}"
            self._target(cr, nx * width, ny * height, self._hold_fraction(), label)
        elif self.phase == "done":
            _rgb(cr, DIM)
            cr.set_line_width(2)
            for nx, ny in POINTS:
                cr.arc(nx * width, ny * height, 8, 0, math.tau)
                cr.stroke()
            if self.miss is not None:
                _text(cr, width / 2, 64, f"dot error about {self.miss * height:.0f} px", 16, DIM)
        elif self.phase == "intro":
            _text(cr, width / 2, height / 2, "Five dots, then the same five again.", 28, INK)
        _text(cr, width / 2, height - 72, self.message, 16, INK)
        cursor = self.correction.point(sample) if self.phase == "done" else gaze_norm(sample)
        self._gaze(cr, cursor, width, height)

    def _hold_fraction(self) -> float:
        if self.phase not in {"device", "check"}:
            return 0.0
        fraction = self.fixation.update(gaze_norm(self.session.latest()), time.monotonic())
        if fraction >= 1.0 and not self.capture_queued:
            self.capture_queued = True
            GLib.idle_add(self._capture)
            return 1.0
        return fraction

    def _target(self, cr, x: float, y: float, fraction: float, label: str) -> None:
        radius = 36
        _rgb(cr, TARGET)
        cr.set_line_width(3)
        cr.arc(x, y, radius, 0, math.tau)
        cr.stroke()
        if fraction > 0:
            _rgb(cr, GOOD)
            cr.set_line_width(6)
            cr.arc(x, y, radius + 10, -math.pi / 2, -math.pi / 2 + math.tau * fraction)
            cr.stroke()
        cr.arc(x, y, 4, 0, math.tau)
        cr.fill()
        _text(cr, x, y + radius + 28, label, 14, DIM)

    def _gaze(self, cr, point: tuple[float, float] | None, width: int, height: int) -> None:
        if point is None:
            return
        _rgb(cr, GAZE)
        cr.set_line_width(3)
        cr.arc(point[0] * width, point[1] * height, 14, 0, math.tau)
        cr.stroke()


class PopStage:
    """Look at the circle until it pops. Thirty seconds."""

    DURATION = 30.0

    def __init__(self, session: GazeSession, correction: ScreenCorrection) -> None:
        self.session = session
        self.correction = correction
        self.started = time.monotonic()
        self.last_draw = self.started
        self.score = 0
        self.pops = 0
        self.target = self._spawn()

    def key(self, keyval: int) -> None:
        return None

    def _spawn(self):
        return {
            "x": random.uniform(0.15, 0.85),
            "y": random.uniform(0.18, 0.78),
            "radius": random.uniform(0.045, 0.08),
            "born": time.monotonic(),
            "held": 0.0,
        }

    def draw(self, cr, width: int, height: int) -> None:
        now = time.monotonic()
        left = self.DURATION - (now - self.started)
        point = self.correction.point(self.session.latest())
        if left <= 0:
            _text(cr, width / 2, height / 2 - 10, str(self.score), 64, GOOD)
            _text(cr, width / 2, height / 2 + 48, f"{self.pops} pops. Esc quits.", 18, DIM)
            self._gaze(cr, point, width, height)
            return
        self._update(point, width, height, now)
        radius = self.target["radius"] * height
        cx, cy = self.target["x"] * width, self.target["y"] * height
        cr.set_source_rgb(0.14, 0.19, 0.14)
        cr.arc(cx, cy, radius, 0, math.tau)
        cr.fill()
        _rgb(cr, TARGET)
        cr.set_line_width(4)
        cr.arc(cx, cy, radius, 0, math.tau)
        cr.stroke()
        if self.target["held"] > 0:
            _rgb(cr, GOOD)
            cr.set_line_width(8)
            cr.arc(cx, cy, radius, -math.pi / 2, -math.pi / 2 + math.tau * min(1.0, self.target["held"] / 0.32))
            cr.stroke()
        _text(cr, 24, 28, str(self.score), 22, INK, "w")
        _text(cr, width - 24, 28, f"{max(0, left):0.0f}s", 18, DIM, "e")
        if point is None:
            _text(cr, width / 2, height - 48, "The tracker does not see your eyes.", 16, WARN)
        self._gaze(cr, point, width, height)

    def _update(self, point, width: int, height: int, now: float) -> None:
        dt = max(0.0, now - self.last_draw)
        self.last_draw = now
        if point is None:
            self.target["held"] = 0.0
            return
        cx, cy = self.target["x"] * width, self.target["y"] * height
        distance = math.hypot(point[0] * width - cx, point[1] * height - cy)
        if distance <= self.target["radius"] * height:
            self.target["held"] += dt
        else:
            self.target["held"] = 0.0
        if self.target["held"] >= 0.32:
            elapsed = now - self.target["born"]
            self.score += max(40, int(180 - elapsed * 40))
            self.pops += 1
            self.target = self._spawn()

    def _gaze(self, cr, point, width: int, height: int) -> None:
        if point is None:
            return
        _rgb(cr, GAZE)
        cr.set_line_width(3)
        cr.arc(point[0] * width, point[1] * height, 10, 0, math.tau)
        cr.stroke()


class TrackerWindow(Gtk.ApplicationWindow):
    def __init__(self, app: Gtk.Application, session: GazeSession, mode: str) -> None:
        super().__init__(application=app, title="Tobii")
        self.session = session
        self.set_decorated(False)
        area = Gtk.DrawingArea()
        area.set_draw_func(self._draw)
        self.set_child(area)
        self.area = area
        keys = Gtk.EventControllerKey()
        keys.connect("key-pressed", self._key)
        self.add_controller(keys)
        note = align_panel(session)
        self.correction = ScreenCorrection()
        if mode == "game":
            self.stage = PopStage(session, self.correction)
        else:
            self.stage = CalibrateStage(session, self.correction, self._play)
            self.stage.message = note + "  Space starts."
        self.fullscreen()
        GLib.timeout_add(16, self._tick)

    def _play(self) -> bool:
        self.stage = PopStage(self.session, self.correction)
        self.area.queue_draw()
        return False

    def _tick(self) -> bool:
        self.area.queue_draw()
        return True

    def _draw(self, _area, cr, width: int, height: int) -> None:
        cr.set_source_rgb(*BG)
        cr.paint()
        self.stage.draw(cr, width, height)

    def _key(self, _controller, keyval: int, _keycode: int, _state) -> bool:
        if keyval == Gdk.KEY_Escape:
            self.close()
            return True
        self.stage.key(keyval)
        self.area.queue_draw()
        return True


def run(mode: str = "calibrate") -> None:
    session = GazeSession()
    try:
        session.connect()
    except OSError as exc:
        raise SystemExit(
            f"Cannot connect to {session.path} ({exc}). Start prefix/bin/tobiifreed first."
        ) from exc

    app = Gtk.Application(application_id="com.local.tobii.play")
    window: dict[str, TrackerWindow] = {}

    def activate(application: Gtk.Application) -> None:
        window["win"] = TrackerWindow(application, session, mode)
        window["win"].present()

    app.connect("activate", activate)
    try:
        app.run(None)
    finally:
        session.close()
