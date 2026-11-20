# cargo-remap

**A new git worktree that builds as fast as the one you copied it from.**

![cargo remap worktree add, then cargo build and cargo check in the new worktree finish at once, with nothing to recompile](https://raw.githubusercontent.com/susitsm/cargo-remap/main/docs/demo.gif)

`cargo-remap` makes commands believe that the current working directory is at another
fixed path. So if you have a project at `~/my-project` and its worktree at `~/worktree`
their `cargo build` builds both as if they were at `~/my-project`. This is achieved using
process local private mount namespaces, so multiple worktrees can run commands at once,
they won't overlap on the filesystem.

Together with `cargo remap worktree add`, which seeds the new worktree with a copy on
write copy of `target/`, and keeps file mtimes, a fresh worktree starts with a warm build
cache with minimal extra disk space usage.

To make everything run correctly (debuggers for example), most commands should be
invoked as `cargo remap exec <command>`, which will run the command with the process
local private mount namespace active. `cargo remap agents` can install a skill ensuring
correct usage by agents.

## Quick start
**`cargo-remap` only works on Linux**

### Install

```text
cargo install cargo-remap
cargo remap doctor         # checks which dependencies are missing, reports whether the filesystem supports copy on write
cargo remap install        # plain `cargo` now goes through `cargo-remap`
```

It should work out of the box on most Linux systems. **Ubuntu 23.10 and later need an
AppArmor profile**. Running `cargo remap doctor` tells you how to set it up.

### Check that it works

Go to a Rust project then:

```text
cargo build                                      # warm up this worktree once
cargo remap worktree add ../feature -b feature   # same arguments as git worktree add
cd ../feature && cargo build                     # nothing to recompile
```

Every worktree is mapped onto the **main worktree's path**, so inside `../feature`,
`cargo remap exec pwd` prints the main worktree's path. Set `CARGO_REMAP_VERBOSE=1`
to print the mapping on every run.

### Running other commands

Since `cargo build` will embed debug info paths as absolute paths, remapping must be
active for debuggers to work properly. Use:

```text
cargo remap exec <command>
```

to run any command with the remapping active.

### Integrating with coding agents

Coding agents won't know how to use cargo-remap without proper instructions.
`cargo remap agents` helps with agent integration.

```text
cargo-remap agents                          # prints the skill for agents
cargo-remap agents --install [--path FILE]  # install it into the current repo and AGENTS.md
```

## Config

`.cargo-remap.toml` in the repo (searched upward from cwd), else
`~/.config/cargo-remap/config.toml`, or the file named by `CARGO_REMAP_CONFIG`.

```toml
path = "/home/me/src/proj"          # where pwd is mapped. Defaults to the main worktree of the repo
target_dir = "~/.cache/proj-target" # optional: exported as CARGO_TARGET_DIR
```

## Worktrees

```text
cargo-remap worktree add ../feature -b feature   # same args as git worktree add
cargo-remap worktree list|remove|...             # passed through to git
```

`add` runs `git worktree add`, then seeds the new worktree from the current one and
finally copies the mtimes of all tracked files that are unchanged relative to the
source worktree, so cargo sees the copied `target/` as fresh. Files that differ, or
that are dirty in the source, keep their new mtime and get rebuilt.

```toml
[worktree]
copy_target = true        # copy target/ (skipped if `target_dir` is set)
copy_gitignored = true    # copy everything gitignored (except target/)
copy_files = [".env"]     # extra paths relative to the repo root that we should always copy
reflink = "auto"          # "auto": CoW if supported, else full copy; "always": CoW or skip; "never": use regular copy
```

## Containers

Running inside containers might need extra permissions. It should generally work with
rootless podman, however docker's default seccomp and AppArmor profiles block the
remapping.

## Tested on

| distro                                                                  | result                                                    |
| ----------------------------------------------------------------------- | --------------------------------------------------------- |
| Debian stable, Fedora, Arch, openSUSE Tumbleweed, Rocky 9, Ubuntu 22.04 | works by default                                          |
| Ubuntu 24.04                                                            | one AppArmor profile needed (the error message prints it) |
| Alpine                                                                  | works after `apk add util-linux`                          |

`tests/distros/run.sh` runs the checks in rootless podman containers.

`tests/distros/vm-ubuntu.sh` boots a real Ubuntu 24.04 VM to test AppArmor's restriction
on user namespaces.
