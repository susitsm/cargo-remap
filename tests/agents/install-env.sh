#!/bin/sh
set -eu
if command -v apt-get >/dev/null; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq && apt-get install -y -qq --no-install-recommends sudo curl ca-certificates gcc libc6-dev bash >/dev/null
    useradd -m -u 1000 -s /bin/bash dev 2>/dev/null || { userdel -r ubuntu 2>/dev/null; useradd -m -u 1000 -s /bin/bash dev; }
elif command -v dnf >/dev/null; then
    dnf install -y -q --allowerasing sudo curl ca-certificates gcc bash >/dev/null
    useradd -m -u 1000 -s /bin/bash dev
elif command -v pacman >/dev/null; then
    pacman -Syu --noconfirm --needed sudo curl ca-certificates gcc bash >/dev/null
    useradd -m -u 1000 -s /bin/bash dev
elif command -v zypper >/dev/null; then
    zypper -q --non-interactive install sudo curl ca-certificates gcc bash >/dev/null
    useradd -m -u 1000 -s /bin/bash dev
elif command -v apk >/dev/null; then
    apk add -q --no-cache sudo curl ca-certificates gcc musl-dev bash
    adduser -D -u 1000 -s /bin/bash dev
fi
echo 'dev ALL=(ALL) NOPASSWD:ALL' >/etc/sudoers.d/dev
