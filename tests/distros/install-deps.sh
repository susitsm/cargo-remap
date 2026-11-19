#!/bin/sh
# Install what the tests need: git, util-linux (unshare, setpriv), curl + a C linker
# for rustup/cargo. Picks the package manager of whatever distro this is.
set -eu
if command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends git util-linux curl ca-certificates gcc libc6-dev >/dev/null
elif command -v dnf >/dev/null; then
    dnf install -y -q --allowerasing git util-linux curl ca-certificates gcc >/dev/null
elif command -v pacman >/dev/null; then
    pacman -Syu --noconfirm --needed git util-linux curl ca-certificates gcc >/dev/null
elif command -v zypper >/dev/null; then
    zypper -q --non-interactive install git util-linux util-linux-systemd curl ca-certificates gcc awk >/dev/null
elif command -v apk >/dev/null; then
    pkgs="git curl ca-certificates gcc musl-dev bash"
    [ "${NO_UTIL_LINUX:-0}" = 1 ] || pkgs="$pkgs util-linux"
    apk add -q --no-cache $pkgs
else
    echo "no known package manager" >&2; exit 1
fi
