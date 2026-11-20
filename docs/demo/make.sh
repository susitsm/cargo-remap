#!/bin/sh
# Regenerate docs/demo.gif from scratch with the current source (needs network once,
# python3 with Pillow, and ffmpeg):
#   docs/demo/make.sh
set -e
H=$(cd "$(dirname "$0")" && pwd); T=$(mktemp -d)
cargo install --path "$H/../.." --locked --quiet && cargo-remap install >/dev/null
sh "$H/setup.sh"
(cd "$HOME" && python3 "$H/record.py" "$T/events.json")
python3 "$H/render.py" "$T/events.json" "$H/../demo.gif" "$T/frames"
rm -rf "$T" "$HOME/.cargo-remap-demo"
