---
name: cargo-remap
description: How to build, test and run commands, and how to create or remove git worktrees, in a repo that uses cargo-remap to map every worktree to one fixed path. Use before running cargo or any path-sensitive command, and whenever creating or removing a worktree.
---

# Working in a cargo-remap repo

This repo uses `cargo-remap`. It bind-mounts the current git worktree at one fixed
path (the "mapped path", by default the main worktree's path) in a private mount
namespace, so every worktree builds from the same absolute path and shares cargo's
fingerprints and caches. Outside the namespace nothing changes.

Find the mapped path and confirm it works: `cargo remap exec pwd`.

## Running commands

- **cargo**: run it as usual if `cargo remap install` was done (check `cargo remap --help`
  works). Otherwise use `cargo remap cargo <args>`, e.g. `cargo remap cargo test`. Never call the
  real cargo binary by absolute path: that skips the mapping and rebuilds from scratch.
- **anything else that depends on paths** (debuggers such as `gdb`, `lldb` and `rr`,
  formatters, linters, `make`, scripts, test runners, code generators): run it as
  `cargo remap exec <command> [args...]`, so it sees the same paths as cargo does.
  Debug info records the mapped path, so a debugger started outside the mapping cannot
  find sources or set breakpoints by file name. Under `exec`, `break src/main.rs:10`
  and source listings work, and they show the files of your current worktree.
- **git**: run it normally. It works under `exec` too (and in build scripts), but needs
  no mapping. Inside the mapping, `git worktree list` shows the main worktree at a
  private path; that is expected.
- No `sudo` or setuid binaries inside `exec` or `cargo remap`; run those outside.

## Paths in output

Paths that start with the mapped path (compiler errors, panics, backtraces, tool
output) refer to files of **your current worktree**, not to the main worktree. To open
or edit such a file, replace the mapped path prefix with your worktree root
(`git rev-parse --show-toplevel`). Editing a file under the mapped path from outside
`cargo remap` changes the main worktree, which is almost never what you want.

## Worktrees

- Create: `cargo remap worktree add <path> [-b <branch>] [<commit-ish>]`
  (same arguments as `git worktree add`). It also copies the current worktree's
  `target/` (copy on write) and any configured ignored files, and restores mtimes of
  unchanged files, so the first build in the new worktree is incremental.
  Do not use plain `git worktree add` for worktrees you will build in.
- Copy from the worktree that already has a fresh build (usually the one you are in).
- List/remove/prune: `cargo remap worktree list|remove <path>|prune` (passed to git).
- Which files get copied is set in `.cargo-remap.toml` under `[worktree]`
  (`copy_target`, `copy_gitignored`, `copy_files`, `reflink`).

## Notes

- Config: `.cargo-remap.toml` (repo root); `path` sets the mapped path, `target_dir`
  shares one target directory between worktrees (builds then take turns on its lock).
- Commands started inside a remapped command have `CARGO_REMAP_ACTIVE=1`; a nested
  `cargo` call goes straight to the real cargo without remapping again. That is
  intended.
- `cargo remap doctor` checks that everything needed is installed and that the mapping
  works here.
- If remapping fails, the error message says what is missing or which setting to change
  (for example the small AppArmor profile that Ubuntu 24.04 needs). Do not work
  around it by calling the real cargo: that rebuilds from scratch in every worktree.
- This file is managed by `cargo remap agents --install`; re-run it to update.
  Don't edit it by hand.
