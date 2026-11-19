//! The text of `cargo-remap --help`.

pub const HELP: &str = "\
cargo-remap: run cargo with the current git worktree bind-mounted at a fixed path

USAGE:
    cargo-remap cargo <cargo args...>    run cargo from the mapped path (e.g. `cargo remap cargo build`)
    cargo-remap worktree add <path> ...  git worktree add, plus copy build state (see below)
    cargo-remap worktree <other> ...     passed through to `git worktree`
    cargo-remap --help | -h | help       this help
    cargo-remap exec <command> [args...]  run any command from the mapped path (e.g. `exec pwd`)
    cargo-remap agents [--install] [--path FILE]
                                         print the guide for coding agents; --install writes it to
                                         FILE (default .agents/skills/cargo-remap/SKILL.md) and
                                         points AGENTS.md at it
    cargo-remap doctor                   check that everything needed is installed and that
                                         namespaces work here (run anywhere, first)
    cargo-remap install [--dir D]        make `cargo` run cargo-remap: symlink D/cargo
                                         (default ~/.local/bin, must precede real cargo in PATH)
    cargo-remap uninstall [--dir D]      remove that symlink

Run as `cargo-remap` or `cargo remap`, only the commands above exist; unknown
commands are an error and are never passed to cargo. After `cargo-remap install`, plain
`cargo <args>` runs the real cargo from the mapped path, with every argument passed
through. `cargo --help` / `cargo help` print a note that this is a cargo wrapper and
point here, before cargo's own help. Reach the commands above as `cargo remap <command>`.

The worktree is bind-mounted in a private user + mount namespace (no root needed),
so every worktree builds from the same absolute path. If you are already at that
path, cargo runs directly.

CONFIG (.cargo-remap.toml searched upward from cwd, else
        ~/.config/cargo-remap/config.toml, or $CARGO_REMAP_CONFIG):
    path        = \"/abs/path\"   path to map to (default: main worktree of the repo)
    target_dir  = \"~/dir\"       exported as CARGO_TARGET_DIR (shared target dir)
    cargo       = \"cargo\"       cargo executable
    agents_path = \"...\"        where `agents --install` writes the guide (relative to repo root)

    [worktree]                    options for `worktree add`
    copy_target  = true           copy target/ from the current worktree
    copy_gitignored = true        copy all gitignored files (except target/)
    copy_files   = []             extra paths, relative to the repo root
    reflink      = \"auto\"       \"auto\" (copy on write if possible, else full copy), \"always\"
                                (copy on write or skip the copy), \"never\"

ENVIRONMENT:
    CARGO_REMAP_CONFIG    path to a config file
    CARGO_REMAP_DRY_RUN   print the command instead of running it
    CARGO_REMAP_VERBOSE   print which path is mapped where on every run
";
