#!/usr/bin/env bash
# One mast, one picture. See tools/pylons/shoot.mjs.
#   tools/pylons/shoot.sh <object-id> [--detail|--foot]
set -euo pipefail
cd "$(dirname "$0")/../.."
id="$1"; shift || true
kind="full"; [[ "${1:-}" == "--detail" ]] && kind="detail"; [[ "${1:-}" == "--foot" ]] && kind="foot"
cam=$(node tools/pylons/shoot.mjs "$id" "$@")
out="screenshots/masts/${id}-${kind}.png"
mkdir -p screenshots/masts
./target/debug/train-sim --line mastparade:audit --time 11:00 --weather clear \
  --camera fly $cam --hud off --frames 90 --screenshot "$out" >/dev/null 2>&1
echo "$out"
