"""Run the demo for real and write events.json: [(t, text)] with ANSI codes.
Usage: python3 record.py events.json   (after setup.sh; cargo remap install done)"""
import json, os, subprocess, sys, time
D = os.path.expanduser("~/.cargo-remap-demo")
ev = []; t0 = time.time()
def emit(s): ev.append((round(time.time() - t0, 3), s))
def sh(cmd, cwd, env=None):
    subprocess.run(cmd, shell=True, cwd=cwd, check=True, capture_output=True, env={**os.environ, **(env or {})})
cur = ""
def prompt(d):
    global cur; cur = d
    emit(f"\x1b[1;34m{d}\x1b[0m$ ")
def typed(s, gap=0.045):
    emit(f"\x1b]cmd;{cur}$ {s}\x07")   # the renderer pins this at the top while cargo scrolls
    for ch in s: emit(ch); time.sleep(gap)
    time.sleep(0.25); emit("\r\n")
def run(cmd, cwd, env=None):
    p = subprocess.Popen(cmd, shell=True, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        env={**os.environ, "CARGO_TERM_COLOR": "always", "CARGO_TERM_PROGRESS_WHEN": "never", **(env or {})}, text=True)
    for line in p.stdout: emit(line.rstrip("\n") + "\r\n")
    p.wait()
def say(s): emit(f"\x1b[2m# {s}\x1b[0m\r\n")

# --- silent reset: warm main worktree
M = f"{D}/myapp"
for w in ("remap",):
    subprocess.run(f"git worktree remove --force ../{w}; git branch -qD {w}", shell=True, cwd=M, capture_output=True)
sh("cargo remap cargo build && cargo remap cargo check", M)
t0 = time.time()   # the silent reset above is not part of the recording

say("main worktree: already built")
prompt("myapp"); typed("cargo build"); run("cargo build", M)
time.sleep(0.8)
say("new worktree with cargo-remap")
prompt("myapp"); typed("cargo remap worktree add ../remap"); run("cargo remap worktree add ../remap 2>&1", M)
prompt("myapp"); typed("cd ../remap && cargo build"); run("cargo build", f"{D}/remap")
prompt("remap"); typed("cargo check"); run("cargo check", f"{D}/remap")
time.sleep(2.5)
json.dump(ev, open(sys.argv[1], "w"))
