#!/bin/sh
# Build the pinned tobiifree daemon with Zig 0.15.2.
# Zig 0.16 does not compile this checkout unchanged.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
zig_bin=${ZIG:-"$HOME/.local/opt/zig-x86_64-linux-0.15.2/zig"}
if [ ! -x "$zig_bin" ]; then
  echo "zig 0.15.2 not found at $zig_bin" >&2
  exit 1
fi
cd "$root/third_party/tobiifree/applications/tobiifreed"
"$zig_bin" build -Doptimize=ReleaseSafe --prefix "$root/prefix"
echo "$root/prefix/bin/tobiifreed"
