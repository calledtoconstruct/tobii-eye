#!/bin/sh
# Install the seat-user ACL for the Tobii Eye Tracker 5. Needs root
# once. Re-run after the rule changes; already-plugged devices are
# retriggered so a replug is not required.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
# 99- sorts after 73-seat-late.rules, so the uaccess tag never became an ACL.
sudo rm -f /etc/udev/rules.d/99-tobii-eyetracker.rules
rule=60-tobii-eyetracker.rules
sudo install -m 0644 "$root/udev/$rule" "/etc/udev/rules.d/$rule"
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=2104 --attr-match=idProduct=0313
sudo udevadm settle
echo "installed /etc/udev/rules.d/$rule"
ls -l /dev/bus/usb/001/002 2>/dev/null || true
getfacl -p /dev/bus/usb/001/002 2>/dev/null || true
