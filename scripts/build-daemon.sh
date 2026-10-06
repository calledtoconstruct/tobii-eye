#!/bin/sh
# Build the Rust tobiifreed daemon.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cargo build --release --manifest-path "$root/daemon/Cargo.toml"
echo "$root/daemon/target/release/tobiifreed"
