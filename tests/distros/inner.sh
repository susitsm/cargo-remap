#!/bin/bash
# Runs inside a distro container as an unprivileged user. Prints one line per check:
#   RESULT|<distro>|<check>|PASS|FAIL|SKIP|<detail>
# Env: DISTRO, LIBC (gnu|musl), EXPECT_FALLBACK=1 (unshare is denied: we must explain,
# not just fail), plus /src (repo, read-only) and /work (scratch, on a reflink fs).
set -u
: "${DISTRO:?}" "${LIBC:?}"
export HOME=/work/home-$DISTRO RUSTUP_HOME=/work/rustup-$LIBC CARGO_HOME=/work/cargo-$LIBC
export CARGO_TARGET_DIR=/work/target-$DISTRO ROOT=/work/root-$DISTRO
export GIT_CONFIG_GLOBAL=$HOME/gitconfig GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
mkdir -p "$HOME" "$ROOT"
git config --global --add safe.directory '*'; git config --global init.defaultBranch main
export PATH=$ROOT/bin:$CARGO_HOME/bin:$PATH

res() { printf 'RESULT|%s|%s|%s|%s\n' "$DISTRO" "$1" "$2" "$(printf %s "${3:-}" | tr '\n|' '  ' | cut -c1-200)"; }
# check <name> <cmd...>: PASS if it succeeds.
check() { local n=$1; shift; local out; if out=$("$@" 2>&1); then res "$n" PASS; else res "$n" FAIL "$out"; fi; }
# expect <name> <regex> <cmd...>: PASS if it succeeds and its output matches.
expect() { local n=$1 re=$2; shift 2; local out; if out=$("$@" 2>&1) && grep -Eq "$re" <<<"$out"; then res "$n" PASS; else res "$n" FAIL "want /$re/ got: $out"; fi; }
# refuse <name> <cmd...>: PASS if it fails.
refuse() { local n=$1; shift; local out; if out=$("$@" 2>&1); then res "$n" FAIL "unexpectedly succeeded"; else res "$n" PASS "$(head -c 80 <<<"$out")"; fi; }

# --- toolchain (rustup is shared between distros of one libc) and cargo-remap itself
if ! command -v rustup >/dev/null; then
    curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --no-modify-path >/dev/null 2>&1
fi
check install-from-source cargo install --path /src --locked --root "$ROOT" --quiet
# Fixtures must use their own target/ (that is what worktree add copies).
unset CARGO_TARGET_DIR
command -v cargo-remap >/dev/null || { res prerequisites FAIL "cargo-remap missing"; exit 0; }

ver=$(unshare --version 2>&1 | head -1); sp=$(setpriv --version 2>&1 | head -1)
if grep -q util-linux <<<"$ver" && grep -q util-linux <<<"$sp"; then res prerequisites PASS "$ver"
else res prerequisites FAIL "unshare: $ver; setpriv: $sp"; fi

# --- fixture: a crate with a path dependency, a main worktree and a linked one
T=/work/t-$DISTRO; rm -rf "$T"; mkdir -p "$T/main/lib/src" "$T/main/src"; cd "$T/main" || exit 1
printf '[package]\nname="app"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="lib"}\n' >Cargo.toml
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' >lib/Cargo.toml
echo 'pub fn f() -> u32 { 1 }' >lib/src/lib.rs
echo 'fn main() { println!("{}", lib::f()); }' >src/main.rs
printf 'target\n' >.gitignore
git init -q && git add -A && git commit -qm init

# --- baseline: plain git worktree (vanilla cargo) rebuilds everything
CARGO_REMAP_ACTIVE=1 cargo build -q 2>/dev/null
git worktree add -q ../plain -b plain
expect plain-worktree-rebuilds 'Compiling lib' bash -c 'cd ../plain && CARGO_REMAP_ACTIVE=1 cargo build 2>&1'

# --- the core promise
if [ "${EXPECT_FALLBACK:-0}" = 1 ]; then
    # unshare(2) is denied by seccomp here: we must explain, not just fail.
    git worktree add -q ../wt -b wt; cd ../wt || exit 1
    expect blocked-userns-explains 'rootless podman' bash -c 'cargo-remap exec pwd 2>&1; true'
    refuse blocked-userns-fails env cargo-remap exec pwd
    expect doctor-explains-block 'rootless podman' bash -c 'cargo-remap doctor 2>&1; true'
    expect worktree-add-refuses-before-creating 'rootless podman' bash -c 'cargo-remap worktree add ../nope -b nope 2>&1; true'
    refuse worktree-add-created-nothing test -e ../nope
    exit 0
fi
expect doctor-ok 'a mount namespace can be created' cargo-remap doctor
expect doctor-works-outside-a-repo 'not in a git repository; run .doctor. inside' bash -c 'cd /tmp && cargo-remap doctor'
# /sub has a mount below it (/sub/inner): a plain bind of it fails in a user namespace
expect doctor-in-dir-with-submount 'a mount namespace can be created' bash -c 'cd /sub && cargo-remap doctor'
expect cargo-runs-outside-a-repo 'cargo [0-9]' bash -c 'cd /tmp && cargo-remap cargo --version'
check worktree-add cargo-remap worktree add ../wt -b wt
cd ../wt || exit 1
expect exec-sees-mapped-path "^$T/main\$" cargo-remap exec pwd
expect exec-uid-unchanged "^$(id -u)\$" cargo-remap exec stat -c %u .
expect exec-sets-active '^1$' cargo-remap exec sh -c 'echo $CARGO_REMAP_ACTIVE'
expect exec-git-works-in-linked-worktree '^wt$' cargo-remap exec git rev-parse --abbrev-ref HEAD
expect exec-git-sees-changes '^\?\? git-probe$' bash -c 'touch git-probe; cargo-remap exec git status --short; rm git-probe'
refuse exec-drops-caps bash -c "cargo-remap exec sh -c 'grep CapEff /proc/self/status' | grep -v '0000000000000000'"
expect warm-build-in-new-worktree 'Finished' bash -c 'cargo-remap cargo build 2>&1'
refuse warm-build-compiles-nothing bash -c 'cargo-remap cargo build 2>&1 | grep -q Compiling'

# --- a repository without commits
mkdir -p /work/t-$DISTRO/empty && (cd /work/t-$DISTRO/empty && git init -q) 
expect no-commits-explained 'no commits yet' bash -c "cd /work/t-$DISTRO/empty && cargo-remap worktree add ../e2 -b e2 2>&1; true"

# --- messages for a missing program (PATH without setpriv)
mkdir -p $T/nosetpriv; for p in sh bash unshare mount git cat env tr grep; do ln -sf "$(command -v $p)" $T/nosetpriv/$p; done
expect missing-program-is-named 'setpriv.*util-linux' bash -c "PATH=$T/nosetpriv $(command -v cargo-remap) exec pwd 2>&1; true"

# --- reflink modes on a filesystem without reflinks (a tmpfs mounted at /rl)
R=/rl/x; rm -rf $R; mkdir -p $R/main/src; cd $R/main
printf '[package]\nname="x"\nversion="0.1.0"\nedition="2021"\n' >Cargo.toml; echo 'fn main(){}' >src/main.rs
printf 'target\n' >.gitignore; git init -q; git add -A; git commit -qm i; CARGO_REMAP_ACTIVE=1 cargo build -q
if cp --reflink=always Cargo.toml $R/probe 2>/dev/null; then res reflink-modes SKIP "/rl supports reflinks here"
else
    printf '[worktree]\nreflink = "always"\n' >$R/always.toml; printf '[worktree]\nreflink = "auto"\n' >$R/auto.toml
    expect reflink-always-warns-and-skips 'could not copy target with copy on write' env CARGO_REMAP_CONFIG=$R/always.toml cargo-remap worktree add ../w1 -b w1
    expect reflink-auto-falls-back 'copied in full' env CARGO_REMAP_CONFIG=$R/auto.toml cargo-remap worktree add ../w2 -b w2
test -d ../w2/target && res reflink-auto-copies-target PASS || res reflink-auto-copies-target FAIL 'no target/ in w2'
fi

# --- cargo wrapper
B=$T/bin; mkdir -p "$B"
check wrapper-install cargo-remap install --dir "$B"
expect wrapper-help 'wrapper' env PATH=$B:$PATH cargo --help
expect wrapper-runs-cargo 'cargo [0-9]' env PATH=$B:$PATH cargo --version
check wrapper-uninstall cargo-remap uninstall --dir "$B"
mkdir -p "$T/foreign"; printf '#!/bin/sh\n' >"$T/foreign/cargo"; chmod +x "$T/foreign/cargo"
expect uninstall-leaves-foreign-cargo "no cargo-remap link" bash -c "cargo-remap uninstall --dir '$T/foreign' && test -f '$T/foreign/cargo' && echo 'no cargo-remap link kept'"
