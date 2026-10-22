# Gaze input

Open-source gaze tools for anyone with a supported tracker. The current
device adapter is USB `2104:0313`. The seat rule also matches `2104:0102`.
More adapters can be added later. Nothing here is tied to one unit or one
screen.

Linux does not ship a driver for the current adapter. On boot the kernel
tries `uvcvideo` and stops, because the eye cameras use a proprietary
format. Nothing is bound after that. Gaze does not come from a video node.
It comes from the vendor-specific interface:

| Interface | Class | Endpoints | Role |
| --- | --- | --- | --- |
| 0 | vendor `ff` | bulk OUT `0x04`, bulk OUT `0x05`, bulk IN `0x83` | control and gaze |
| 1 | UVC control | none on alt 0 | eye cameras, proprietary format |
| 2 | UVC stream | bulk IN `0x82` | same, rejected by the kernel |

The public Tobii Stream Engine samples are Windows-only. The daemon in
`daemon/` is a GPL-3.0-only Rust translation of
[tobiifree](https://github.com/Aetherall/tobiifree). It claims the USB
device with libusb and publishes gaze on a Unix socket. The Python package
in this directory is the client later software should import. It does not
link the GPL daemon. A license for that client has not been chosen yet.

## Run

The seat rule is `/etc/udev/rules.d/60-tobii-eyetracker.rules`. It has to sort before `73-seat-late.rules`, which is what turns `TAG+=uaccess` into an access list. The packaged rule uses that seat access list. `udev/60-tobii-eyetracker.rules` also sets group `wheel`.

```sh
cargo build --release --manifest-path daemon/Cargo.toml
./daemon/target/release/tobiifreed --ws 127.0.0.1:7081
```

In another terminal:

```sh
tobii-gaze --count 30
python -m tobii_input --json
tobii-calibrate
tobii-pop
```

`tobii-calibrate` is a fullscreen GTK window. Space starts five dots. A dot records only after your gaze moves to it and holds still, or immediately if you press Space while you are looking at it. The same five dots then run again. That second pass measures where the cursor actually sits and saves a screen correction in `~/.config/tobii/screen_warp.json`. Pop uses that correction. Esc quits either one.

The client reads the focused monitor's physical size. When the device plane is still the daemon placeholder, 1500 by 1000 mm, it writes a plane for that monitor to `~/.config/tobii.json`. The current adapter sits under the panel: the panel is centered on X, and its bottom edge is 10 mm above the sensor.

Install the package, or put `src` on `PYTHONPATH`, before `python -m tobii_input`. `tobii-gaze` is the console script.

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
rule, and a disabled user service (`tobiifreed --ws 127.0.0.1:7081`).
`python-tobii-input` is the client, the calibration window, and Pop. The
daemon package is https://github.com/omacom/omarchy-pkgs/pull/817. The client
package stays in this repository until a license is chosen. The gaze desktop
design is in `docs/omarchy-gaze.md`.
