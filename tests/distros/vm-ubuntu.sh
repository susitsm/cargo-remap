#!/bin/bash
# Real Ubuntu 24.04 VM (QEMU/KVM): the only place the AppArmor restriction on
# unprivileged user namespaces can be tested. Runs twice:
#   restricted=1 (Ubuntu's default): exec must fail with a message that prints the
#                AppArmor profile; after running the commands in that message, warm builds work.
#   restricted=0:                    everything works directly.
# Needs: qemu-system-x86_64 with KVM, genisoimage, ssh. Downloads ~600 MB once.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$HERE/../.." && pwd)
WORK=${WORK:-$HOME/.cache/cargo-remap-distros}/vm; mkdir -p "$WORK"; cd "$WORK"
IMG=noble-server-cloudimg-amd64.img; PORT=${PORT:-2222}
[ -f base.img ] || { wget -q -O base.img.part "https://cloud-images.ubuntu.com/noble/current/$IMG" && mv base.img.part base.img; }
[ -f key ] || ssh-keygen -q -t ed25519 -N '' -f key
rm -f disk.qcow2; qemu-img create -q -f qcow2 -F qcow2 -b base.img disk.qcow2 20G
mkdir -p seed; cat >seed/user-data <<YAML
#cloud-config
users:
  - name: tester
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys: ["$(cat key.pub)"]
YAML
printf 'instance-id: cr-test\nlocal-hostname: cr-test\n' >seed/meta-data
genisoimage -quiet -output seed.iso -volid cidata -joliet -rock seed/user-data seed/meta-data
qemu-system-x86_64 -enable-kvm -cpu host -m 4096 -smp 4 -display none -daemonize -pidfile qemu.pid \
  -drive file=disk.qcow2,if=virtio -drive file=seed.iso,if=virtio,format=raw \
  -nic user,hostfwd=tcp:127.0.0.1:$PORT-:22
trap 'kill $(cat qemu.pid) 2>/dev/null || true' EXIT
SSH=(ssh -i key -p $PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR tester@127.0.0.1)
for _ in $(seq 120); do "${SSH[@]}" true 2>/dev/null && break; sleep 3; done
"${SSH[@]}" cloud-init status --wait >/dev/null 2>&1 || true
tar -C "$REPO" --exclude=target --exclude=.git -cf - . | "${SSH[@]}" 'mkdir -p src && tar -C src -xf -'
"${SSH[@]}" 'sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq git curl gcc >/dev/null'
for restricted in 1 0; do
  "${SSH[@]}" "sudo sysctl -q -w kernel.apparmor_restrict_unprivileged_userns=$restricted"
  "${SSH[@]}" "RESTRICTED=$restricted bash -s" <"$HERE/vm-inner.sh" | grep '^RESULT|' | sed "s/|ubuntu-24.04-vm|/|ubuntu-24.04-vm-restricted$restricted|/" | tee -a results-vm.txt
done
