#!/bin/bash
# Runs inside the VM as the normal user. Uses the same RESULT format as inner.sh.
set -u
export HOME=/home/tester; cd "$HOME"
D=ubuntu-24.04-vm
res() { printf 'RESULT|%s|%s|%s|%s\n' "$D" "$1" "$2" "$(printf %s "${3:-}" | tr '\n|' '  ' | cut -c1-200)"; }
expect() { local n=$1 re=$2; shift 2; local out; if out=$("$@" 2>&1) && grep -Eq "$re" <<<"$out"; then res "$n" PASS; else res "$n" FAIL "want /$re/ got: $out"; fi; }
refuse() { local n=$1; shift; local out; if out=$("$@" 2>&1); then res "$n" FAIL "unexpectedly succeeded"; else res "$n" PASS; fi; }
command -v rustup >/dev/null || [ -x ~/.cargo/bin/rustup ] || curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal >/dev/null 2>&1
export PATH=$HOME/.cargo/bin:$PATH
cargo install --path src --locked --quiet 2>/dev/null && res install-from-source PASS || res install-from-source FAIL
R=$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns); res apparmor-restrict-sysctl-is-$RESTRICTED $([ "$R" = "$RESTRICTED" ] && echo PASS || echo FAIL)
T=$HOME/t; rm -rf $T; mkdir -p $T/main/src; cd $T/main
printf '[package]\nname="x"\nversion="0.1.0"\nedition="2021"\n' >Cargo.toml; echo 'fn main(){println!("hi")}' >src/main.rs
printf 'target\n' >.gitignore; git init -q; git add -A; git -c user.name=t -c user.email=t@t commit -qm i
cargo build -q
git worktree add -q ../plain -b plain      # plain git always works; used for the blocked checks
if [ "$RESTRICTED" = 1 ]; then
    expect apparmor-message-has-the-fix 'sudo apparmor_parser -r /etc/apparmor.d/cargo-remap-unshare' bash -c 'cd ../plain && cargo-remap exec pwd 2>&1; true'
    refuse apparmor-blocks-exec bash -c 'cd ../plain && cargo-remap exec pwd'
    expect worktree-add-refuses-before-creating 'cargo-remap-unshare' bash -c 'cargo-remap worktree add ../wt -b wt 2>&1; true'
    refuse worktree-add-created-nothing test -e ../wt
    # follow the message's advice, verbatim
    (cd ../plain && cargo-remap exec pwd 2>&1 | sed -n '/^sudo tee/,/^sudo apparmor_parser/p' >/tmp/fix.sh); sh -eu /tmp/fix.sh
fi
cargo-remap worktree add ../wt -b wt >/dev/null 2>&1; cd ../wt
expect exec-sees-mapped-path "^$T/main\$" cargo-remap exec pwd
expect warm-build 'Finished' cargo-remap cargo build
refuse warm-build-compiles-nothing bash -c 'cargo-remap cargo build 2>&1 | grep -q Compiling'
