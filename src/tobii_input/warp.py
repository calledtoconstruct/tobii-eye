"""Screen correction on top of the tracker's own calibration.

The device maps gaze onto the display plane. On this laptop that plane is
flat and untilted, so the cursor is still off in parts of the panel after
the device accepts a calibration. A homography fitted from "where the
cursor sat" to "where the dot was" is the extra mapping Pop uses.
"""

from __future__ import annotations

import json
import math
import os

WARP_PATH = os.path.expanduser("~/.config/tobii/screen_warp.json")

Point = tuple[float, float]
Matrix = tuple[tuple[float, float, float], tuple[float, float, float], tuple[float, float, float]]


def apply_homography(homography: Matrix, point: Point) -> Point:
    x, y = point
    row0, row1, row2 = homography
    weight = row2[0] * x + row2[1] * y + row2[2]
    if abs(weight) < 1e-9:
        return point
    return (
        (row0[0] * x + row0[1] * y + row0[2]) / weight,
        (row1[0] * x + row1[1] * y + row1[2]) / weight,
    )


def fit_homography(pairs: list[tuple[Point, Point]]) -> Matrix | None:
    """Least-squares homography from measured gaze to the screen dot.

    Four pairs determine a homography. Five pairs, the calibration set,
    are solved together so one noisy dot cannot swing a corner.
    """
    if len(pairs) < 4:
        return None
    rows: list[list[float]] = []
    targets: list[float] = []
    for (x, y), (u, v) in pairs:
        rows.append([x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y])
        targets.append(u)
        rows.append([0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y])
        targets.append(v)
    normal = [[0.0] * 8 for _ in range(8)]
    normal_target = [0.0] * 8
    for row, target in zip(rows, targets):
        for r in range(8):
            normal_target[r] += row[r] * target
            for c in range(8):
                normal[r][c] += row[r] * row[c]
    solved = _solve(normal, normal_target)
    if solved is None:
        return None
    h0, h1, h2, h3, h4, h5, h6, h7 = solved
    return ((h0, h1, h2), (h3, h4, h5), (h6, h7, 1.0))


def mean_miss(homography: Matrix, pairs: list[tuple[Point, Point]]) -> float:
    if not pairs:
        return 0.0
    total = 0.0
    for source, dest in pairs:
        x, y = apply_homography(homography, source)
        total += math.hypot(x - dest[0], y - dest[1])
    return total / len(pairs)


def save_warp(homography: Matrix, path: str = WARP_PATH) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump({"homography": [list(row) for row in homography]}, fh, indent=2)
        fh.write("\n")


def load_warp(path: str = WARP_PATH) -> Matrix | None:
    try:
        with open(path, encoding="utf-8") as fh:
            raw = json.load(fh)["homography"]
    except (OSError, KeyError, json.JSONDecodeError, TypeError):
        return None
    if len(raw) != 3 or any(len(row) != 3 for row in raw):
        return None
    return tuple(tuple(float(value) for value in row) for row in raw)  # type: ignore[return-value]


def _solve(matrix: list[list[float]], rhs: list[float]) -> list[float] | None:
    size = len(rhs)
    augmented = [matrix[row][:] + [rhs[row]] for row in range(size)]
    for col in range(size):
        pivot = max(range(col, size), key=lambda row: abs(augmented[row][col]))
        if abs(augmented[pivot][col]) < 1e-12:
            return None
        augmented[col], augmented[pivot] = augmented[pivot], augmented[col]
        scale = augmented[col][col]
        for col2 in range(col, size + 1):
            augmented[col][col2] /= scale
        for row in range(size):
            if row == col:
                continue
            factor = augmented[row][col]
            for col2 in range(col, size + 1):
                augmented[row][col2] -= factor * augmented[col][col2]
    return [augmented[row][size] for row in range(size)]


class Fixation:
    """Tracks a still gaze, and ignores a still gaze that has not moved yet.

    The old capture fired as soon as the eyes were visible. That recorded
    the previous dot's gaze against the new dot. A later dot is accepted
    only after the gaze has moved away from the last accepted sample and
    then stayed still.
    """

    STABLE = 0.03
    MOVE = 0.08
    HOLD = 0.75

    def __init__(self) -> None:
        self.anchor: Point | None = None
        self._still_at: Point | None = None
        self._since: float | None = None
        self.samples: list[Point] = []

    def reset_point(self) -> None:
        self._still_at = None
        self._since = None
        self.samples = []

    def update(self, point: Point | None, now: float) -> float:
        if point is None:
            self.reset_point()
            return 0.0
        if self._still_at is None or _distance(point, self._still_at) > self.STABLE:
            self._still_at = point
            self._since = now
            self.samples = [point]
            return 0.0
        self.samples.append(point)
        if self.anchor is not None and _distance(point, self.anchor) < self.MOVE:
            return 0.0
        assert self._since is not None
        return min(1.0, (now - self._since) / self.HOLD)

    def median(self) -> Point | None:
        if not self.samples:
            return None
        xs = sorted(sample[0] for sample in self.samples)
        ys = sorted(sample[1] for sample in self.samples)
        mid = len(xs) // 2
        return xs[mid], ys[mid]


def _distance(a: Point, b: Point) -> float:
    return math.hypot(a[0] - b[0], a[1] - b[1])
