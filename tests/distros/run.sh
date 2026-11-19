#!/bin/bash
# Test cargo-remap on several distros in rootless podman containers.
#   tests/distros/run.sh [distro...]     (default: all)
# Each distro: build an image with the distro's userland, then, as your own uid with the
# repo mounted read-only, `cargo install --path` cargo-remap and run inner.sh. A second
# run with `unshare(2)` denied by seccomp checks the error message.
# Scratch space defaults to ~/.cache/cargo-remap-distros: put it on a reflink filesystem
# (btrfs, XFS) to exercise real copy on write copies.
set -u
HERE=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$HERE/../.." && pwd)
WORK=${WORK:-$HOME/.cache/cargo-remap-distros}; ENGINE=${ENGINE:-podman}
mkdir -p "$WORK" "$WORK/subinner"; OUT=$WORK/results.txt; : >"$OUT"

# name|base image|libc|NO_UTIL_LINUX
ALL=(
 "debian|docker.io/library/debian:stable-slim|gnu|0"
 "ubuntu-24.04|docker.io/library/ubuntu:24.04|gnu|0"
 "ubuntu-22.04|docker.io/library/ubuntu:22.04|gnu|0"
 "fedora|registry.fedoraproject.org/fedora:latest|gnu|0"
 "rocky-9|docker.io/library/rockylinux:9|gnu|0"
 "arch|docker.io/library/archlinux:latest|gnu|0"
 "opensuse-tumbleweed|registry.opensuse.org/opensuse/tumbleweed:latest|gnu|0"
 "alpine|docker.io/library/alpine:latest|musl|0"
 "alpine-busybox|docker.io/library/alpine:latest|musl|1"
)
cat >"$WORK/no-unshare.json" <<'JSON'
{"defaultAction":"SCMP_ACT_ALLOW","syscalls":[{"names":["unshare"],"action":"SCMP_ACT_ERRNO","errnoRet":1}]}
JSON

run_one() { # name image libc nous fallback
  local name=$1 image=$2 libc=$3 nous=$4 fb=$5 sec=()
  [ "$fb" = 1 ] && sec=(--security-opt "seccomp=$WORK/no-unshare.json")
  "$ENGINE" run --rm --userns=keep-id --security-opt label=disable "${sec[@]}" \
    -v "$REPO:/src:ro" -v "$HERE/inner.sh:/inner.sh:ro" -v "$WORK:/work" --tmpfs /rl:mode=1777,exec -v "$WORK/subinner:/sub/inner" -w /work \
    -e DISTRO="$name" -e LIBC="$libc" -e EXPECT_FALLBACK="$fb" -e TERM=dumb \
    "localhost/cr-test/$name" bash /inner.sh 2>&1 | tee -a "$WORK/log-$name$([ "$fb" = 1 ] && echo -fallback).txt" | grep '^RESULT|' >>"$OUT"
}

sel=("$@")
for entry in "${ALL[@]}"; do
  IFS='|' read -r name image libc nous <<<"$entry"
  if [ ${#sel[@]} -gt 0 ] && [[ ! " ${sel[*]} " == *" $name "* ]]; then continue; fi
  echo "== $name"
  if ! "$ENGINE" build -q -t "localhost/cr-test/$name" --build-arg BASE="$image" --build-arg NO_UTIL_LINUX="$nous" \
        -f "$HERE/Containerfile" "$HERE" >"$WORK/build-$name.txt" 2>&1; then
    echo "RESULT|$name|image-build|FAIL|$(tail -1 "$WORK/build-$name.txt")" >>"$OUT"; continue
  fi
  run_one "$name" "$image" "$libc" "$nous" 0
  run_one "$name" "$image" "$libc" "$nous" 1
done
python3 "$HERE/summarize.py" "$OUT"
