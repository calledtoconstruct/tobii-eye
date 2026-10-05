# Tobii Eye Tracker 5 on this machine

USB shows a Tobii Eye Tracker 5 on this machine: Tobii AB EyeChip,
serial `IS5FF-100204240211`, id `2104:0313`, high speed.

Linux does not ship a driver for it. On boot the kernel tries `uvcvideo`
and stops:

```
uvcvideo 1-2:1.2: Unknown video format e39e1ba2-1599-3248-8728-e1b25923a611
uvcvideo 1-2:1.2: No supported video formats found.
uvcvideo 1-2:1.1: probe with driver uvcvideo failed with error -22
```

Nothing is bound after that. The integrated webcam is a different device
and is unrelated. Gaze does not come out of that video node. It comes from
the vendor-specific interface:

| Interface | Class | Endpoints | Role |
| --- | --- | --- | --- |
| 0 | vendor `ff` | bulk OUT `0x04`, bulk OUT `0x05`, bulk IN `0x83` | control and gaze |
| 1 | UVC control | none on alt 0 | eye cameras, proprietary format |
| 2 | UVC stream | bulk IN `0x82` | same, rejected by the kernel |

The public Tobii Stream Engine samples are Windows-only. The working Linux
implementation for this exact id is [tobiifree](https://github.com/Aetherall/tobiifree)
(GPL-3.0), pinned in `third_party/tobiifree` at `ad82906`. Its daemon owns
the USB device and publishes gaze on a Unix socket. The Python package in
this directory is the client later software should import. It does not link
the GPL daemon. A license for that client has not been chosen yet.

This repository does not contain the tobiifree checkout. From a fresh clone:

```sh
git clone https://github.com/Aetherall/tobiifree third_party/tobiifree
git -C third_party/tobiifree checkout ad82906a9c97f03be8bfcfeff9453198f6feb7a0
patch -d third_party/tobiifree -p1 < packaging/tobiifreed/reconnect.patch
```

## Run

The seat rule is `/etc/udev/rules.d/60-tobii-eyetracker.rules`. It has to sort before `73-seat-late.rules`, which is what turns `TAG+=uaccess` into an access list. With that in place the device node is group `wheel` and this user can open it.

```sh
./prefix/bin/tobiifreed
```

In another terminal:

```sh
tobii-gaze --count 30
python -m tobii_input --json
tobii-calibrate
tobii-pop
```

`tobii-calibrate` is a fullscreen GTK window. Space starts five dots. A dot records only after your gaze moves to it and holds still, or immediately if you press Space while you are looking at it. The same five dots then run again. That second pass measures where the cursor actually sits and saves a screen correction in `~/.config/tobii/screen_warp.json`. Pop uses that correction. Esc quits either one.

The first launch sets the display plane to this panel, 290 by 170 mm, centered 10 mm above the sensor, and writes that to `~/.config/tobii.json`. The daemon's built-in plane is 1500 by 1000 mm, which does not match this screen.

`python -m tobii_input` finds the package through
`~/.local/lib/python3.14/site-packages/tobii-input.pth`. `tobii-gaze` is on
`~/.local/bin`.

The socket is `$XDG_RUNTIME_DIR/tobiifreed/gaze.sock`. The daemon keeps that socket open if the tracker is unplugged, waits for it, and runs the USB handshake again when it returns. A quiet stream is not a gaze point: clients drop a sample that is more than half a second old, and they reconnect if the daemon process itself restarts.

```python
from tobii_input import TobiiClient

with TobiiClient() as tobii:
    for sample in tobii.samples():
        if sample.gaze_xy is None:
            continue
        x, y = sample.gaze_xy  # filtered binocular point, 0..1 on the display area
        break
```

`gaze_xy` is the top-left origin, normalized to the tracker's display plane.
`validity_left` and `validity_right` are `0` when that eye is detected and
`4` when it is not. Positions in millimeters use the tracker frame: X right,
Y up, Z toward the user, origin on the IR array.

`tobiifreed` reads `~/.config/tobii.json` for the display size. Without that
file it keeps whatever plane the device already has. `--init-config` writes
a starting file. `tobii-calibrate` sends the calibration commands on the
socket, then writes the screen correction Pop reads.

Prepared packages live in `packaging/`. `tobiifreed` is the daemon, the seat
rule, and a disabled user service (`tobiifreed --ws 127.0.0.1:7081`). The
packaged rule relies on the seat access list. The rule already installed on
this machine also sets group `wheel`. `python-tobii-input` is the client, the
calibration window, and Pop. Neither package has been submitted. The gaze
desktop design is in `docs/omarchy-gaze.md`.

The daemon binary is built with Zig 0.15.2 from
`~/.local/opt/zig-x86_64-linux-0.15.2`. Zig 0.16 no longer compiles this
checkout unchanged.
