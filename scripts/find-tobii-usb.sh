#!/bin/sh
# Print the ACL for each plugged-in Tobii adapter (2104:0313 or 2104:0102).
# Device nodes come from sysfs busnum/devnum. There is no fixed bus path.
# TOBII_SYSFS overrides /sys. TOBII_DEV_ROOT overrides /dev.
# When nothing is plugged in, say so and exit 0.
set -eu

sysfs=${TOBII_SYSFS:-/sys}
dev_root=${TOBII_DEV_ROOT:-/dev}
devices=$sysfs/bus/usb/devices
found=0

strip_zeros() {
  v=$1
  while [ "${#v}" -gt 1 ] && [ "${v#0}" != "$v" ]; do
    v=${v#0}
  done
  printf '%s' "$v"
}

if [ -d "$devices" ]; then
  for dev in "$devices"/*; do
    [ -d "$dev" ] || continue
    [ -f "$dev/idVendor" ] || continue
    [ -f "$dev/idProduct" ] || continue
    [ -f "$dev/busnum" ] || continue
    [ -f "$dev/devnum" ] || continue
    vendor=$(tr -d '[:space:]' < "$dev/idVendor" | tr 'ABCDEF' 'abcdef')
    product=$(tr -d '[:space:]' < "$dev/idProduct" | tr 'ABCDEF' 'abcdef')
    [ "$vendor" = "2104" ] || continue
    case $product in
      0313|0102) ;;
      *) continue ;;
    esac
    bus=$(tr -d '[:space:]' < "$dev/busnum")
    num=$(tr -d '[:space:]' < "$dev/devnum")
    case $bus in
      ''|*[!0-9]*) continue ;;
    esac
    case $num in
      ''|*[!0-9]*) continue ;;
    esac
    bus=$(strip_zeros "$bus")
    num=$(strip_zeros "$num")
    node=$(printf '%s/bus/usb/%03d/%03d' "$dev_root" "$bus" "$num")
    found=1
    printf '%s\n' "$node"
    getfacl -p -- "$node" || true
  done
fi

if [ "$found" -eq 0 ]; then
  echo "no Tobii device plugged in (idVendor 2104, idProduct 0313 or 0102)"
fi
exit 0
