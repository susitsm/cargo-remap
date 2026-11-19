#!/bin/bash
# Disposable environments for "first-time user" test agents.
#   env.sh up [name...]     build (cached) and start fresh environments (default: all)
#   env.sh down             remove them all
#   env.sh cmd <name>       print the command prefix that runs a shell command there
# Containers share the host kernel; `ubuntu-24.04-vm` is a QEMU VM with Ubuntu's
# default AppArmor restriction on user namespaces.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd); REPO=$(cd "$HERE/../.." && pwd)
WORK=${WORK:-$HOME/.cache/cargo-remap-agents}; mkdir -p "$WORK"
declare -A BASE=(
  [debian]=docker.io/library/debian:stable-slim [ubuntu-24.04]=docker.io/library/ubuntu:24.04
  [ubuntu-22.04]=docker.io/library/ubuntu:22.04 [fedora]=registry.fedoraproject.org/fedora:latest
  [rocky-9]=docker.io/library/rockylinux:9 [arch]=docker.io/library/archlinux:latest
  [opensuse]=registry.opensuse.org/opensuse/tumbleweed:latest [alpine]=docker.io/library/alpine:latest )
ALL=(debian ubuntu-24.04 ubuntu-22.04 fedora rocky-9 arch opensuse alpine ubuntu-24.04-vm)
PORT=2222; SSHOPT=(-i "$WORK/vm/key" -p $PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

up_container() {
  local n=$1; podman rm -f "crenv-$n" >/dev/null 2>&1 || true
  podman build -q -t "localhost/crenv/$n" --build-arg BASE="${BASE[$n]}" -f "$HERE/Containerfile" "$HERE" >"$WORK/build-$n.log" 2>&1 \
    || { echo "build failed: $n (see $WORK/build-$n.log)"; return 1; }
  rm -rf "$WORK/work-$n"; mkdir -p "$WORK/work-$n"
  podman run -d --name "crenv-$n" --userns=keep-id --user 1000 --security-opt label=disable \
    -v "$WORK/work-$n:/home/dev/work" localhost/crenv/$n sleep infinity >/dev/null
  podman exec -u dev "crenv-$n" mkdir -p /home/dev/cargo-remap
  tar -C "$REPO" --exclude=target --exclude=.git -cf - . | podman exec -i -u dev "crenv-$n" tar -C /home/dev/cargo-remap -xf -
}
up_vm() {
  mkdir -p "$WORK/vm"; cd "$WORK/vm"; down_vm
  [ -f base.img ] || cp "$HOME/.cache/cargo-remap-distros/vm/base.img" base.img
  [ -f key ] || ssh-keygen -q -t ed25519 -N '' -f key
  rm -f disk.qcow2; qemu-img create -q -f qcow2 -F qcow2 -b base.img disk.qcow2 20G
  mkdir -p seed; printf '#cloud-config\nusers:\n  - name: tester\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    shell: /bin/bash\n    ssh_authorized_keys: ["%s"]\n' "$(cat key.pub)" >seed/user-data
  printf 'instance-id: cr-agent\nlocal-hostname: cr-agent\n' >seed/meta-data
  genisoimage -quiet -output seed.iso -volid cidata -joliet -rock seed/user-data seed/meta-data
  qemu-system-x86_64 -enable-kvm -cpu host -m 4096 -smp 4 -display none -daemonize -pidfile qemu.pid \
    -drive file=disk.qcow2,if=virtio -drive file=seed.iso,if=virtio,format=raw -nic user,hostfwd=tcp:127.0.0.1:$PORT-:22
  for _ in $(seq 120); do ssh "${SSHOPT[@]}" tester@127.0.0.1 true 2>/dev/null && break; sleep 3; done
  ssh "${SSHOPT[@]}" tester@127.0.0.1 'cloud-init status --wait >/dev/null 2>&1; sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl gcc >/dev/null; curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal >/dev/null 2>&1; mkdir -p ~/cargo-remap; echo PATH=/home/tester/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin | sudo tee /etc/environment >/dev/null'
  tar -C "$REPO" --exclude=target --exclude=.git -cf - . | ssh "${SSHOPT[@]}" tester@127.0.0.1 'tar -C ~/cargo-remap -xf -'
}
down_vm() { [ -f "$WORK/vm/qemu.pid" ] && kill "$(cat "$WORK/vm/qemu.pid")" 2>/dev/null && sleep 2 || true; }

case ${1:-} in
  up) shift; sel=("$@"); [ ${#sel[@]} -gt 0 ] || sel=("${ALL[@]}")
      for n in "${sel[@]}"; do [ "$n" = ubuntu-24.04-vm ] || echo "$n"; done | xargs -P4 -I{} bash -c "$(declare -f up_container); BASE_DECL=1; $(declare -p BASE HERE REPO WORK); up_container {}"
      [[ " ${sel[*]} " == *" ubuntu-24.04-vm "* ]] && up_vm
      echo ready ;;
  down) for n in "${ALL[@]}"; do podman rm -f "crenv-$n" >/dev/null 2>&1 || true; done; down_vm ;;
  cmd) n=$2
      if [ "$n" = ubuntu-24.04-vm ]; then echo "ssh -i $WORK/vm/key -p $PORT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR tester@127.0.0.1"
      else echo "podman exec -u dev -w /home/dev crenv-$n bash -c"; fi ;;
  *) sed -n 2,6p "$0"; exit 1 ;;
esac
