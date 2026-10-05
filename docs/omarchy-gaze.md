# Gaze operation of an Omarchy session

Saved for a later implementation. This starts after `tobiifreed` is an AUR
package and an Omarchy package. It is a desktop feature on top of that
daemon. It is not part of the package work.

## Interaction

Gaze moves a cursor. Holding still on a target selects it. The targets are
large. Looking away cancels. A short sound confirms a selection. A different
short sound confirms a cancellation.

The first version uses those two gestures only. The tracker reports a point,
a pupil diameter, and a per-eye validity flag (`0` seen, `4` not seen). It
does not report blink, wink, or turn-away as events. A blink can be inferred
later from a brief loss of both eyes. A wink can be inferred later from the
loss of one eye. Both wait until a person records them on this device,
because a tracking dropout looks like a blink.

Looking away means both eyes stay unseen, or the point leaves the panel, for
long enough that it cannot be confused with a momentary dropout.

## Pieces

A user-session bridge starts after `tobiifreed`. It reads the gaze socket,
applies `~/.config/tobii/screen_warp.json` when that file exists, and decides
dwell and look-away. It feeds a virtual pointer that Hyprland already
accepts, so a selection click lands in the focused window.

An Omarchy shell plugin draws the gaze cursor, the dwell progress, and the
large targets. The bar stays a thin strip, so gaze mode does not aim at those
widgets. It summons the existing menu through `omarchy-shell` and shows the
same entries as tall rows. A dwell on a row runs that row's existing action.
Looking away closes the menu.

The greeter starts before this user service. Reaching the login screen by
gaze means an automatic login, or a later change to the greeter. Windows
programs that expect Tobii's own runtime are outside this work.
