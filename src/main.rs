#![doc = include_str!("../README.md")]

use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use serde::Deserialize;

mod agents;
mod backend;
mod cli;
mod diagnose;
mod worktree;

use cli::Action;
use worktree::WorktreeConfig;

fn main() {
    let args = env::args().skip(1).collect();
    match cli::parse(cli::invoked_as_cargo(), args) {
        Action::Help => print!("{}", HELP),
        Action::CargoHelp(arg) => cargo_help(&arg),
        Action::Install(args) => install(&args),
        Action::Uninstall(args) => uninstall(&args),
        Action::Worktree(args) => worktree::run(&args),
        Action::Doctor => doctor(),
        Action::Agents(args) => agents::run(&args, load_config_here().agents_path.as_deref()),
        Action::Run { program, args } => run_remapped(program, &args),
    }
}

/// `cargo-remap doctor`: run the checks a real run does, and report. Works anywhere;
/// inside a git repository it also shows how that worktree is mapped.
fn doctor() {
    let cwd = env::current_dir().unwrap_or_else(|e| die(format!("no current dir: {e}")));
    let cfg = load_config(&cwd);
    let cargo = resolve_cargo(&cfg);
    // Every program in one pass, so one message lists everything that is missing.
    diagnose::require(&["git", "cp", "unshare", "setpriv", "mount", "sh"]);
    println!("ok: cargo is {cargo}");
    println!("ok: git, cp, unshare, setpriv and mount are installed");
    let in_repo = git(&cwd, &["rev-parse", "--show-toplevel"]).is_some();
    if in_repo || cfg.path.is_some() {
        let m = backend::preflight(&cfg, &cwd);
        println!("ok: a mount namespace can be created here");
        println!(
            "ok: this worktree ({}) is mapped to {}",
            m.src.display(),
            m.dst.display()
        );
        if git(&m.src, &["rev-parse", "--verify", "-q", "HEAD"]).is_none() {
            println!(
                "note: this repository has no commits yet, and `worktree add` needs one:\n\
                 \x20     git add -A && git commit -m \"first commit\""
            );
        }
        if m.src.join("Cargo.toml").exists()
            && git(&m.src, &["check-ignore", "-q", "target"]).is_none()
        {
            println!(
                "note: `target/` is not in .gitignore, so it would be committed:\n\
                 \x20     echo /target >> .gitignore"
            );
        }
        if m.src == m.dst {
            println!(
                "    (this is the main worktree, so nothing changes here; every other\n\
                 \x20   worktree of the repository is mapped onto this path)"
            );
        }
    } else {
        backend::preflight_anywhere(&cwd);
        println!("ok: a mount namespace can be created here");
        println!(
            "note: this directory is not in a git repository; run `doctor` inside your project to see its mapping"
        );
    }
    let probe_dir = git(&cwd, &["rev-parse", "--show-toplevel"]).map_or(cwd.clone(), Into::into);
    if worktree::reflink_works(&probe_dir) {
        println!(
            "ok: copy on write copies work in {}, so new worktrees get target/ instantly",
            probe_dir.display()
        );
    } else {
        println!(
            "note: {} has no copy on write copies, so a new worktree gets a full copy\n\
             \x20     of target/ (works, but takes longer and more space; btrfs and XFS are instant)",
            probe_dir.display()
        );
    }
    if wrapper_active() {
        println!("ok: plain `cargo` runs through cargo-remap");
    } else {
        println!(
            "note: plain `cargo` does not run through cargo-remap (`command -v cargo` shows the real one).\n\
             \x20     Use `cargo-remap cargo ...`, or run `cargo-remap install` and follow its PATH advice"
        );
    }
}

/// `cargo --help` as a stand-in: say that this is a wrapper, then show cargo's help.
fn cargo_help(arg: &str) {
    println!(
        "note: this `cargo` is a wrapper (cargo-remap) that runs cargo from a fixed\n\
         worktree path. See `cargo remap --help` for details.\n"
    );
    let _ = Command::new(resolve_cargo(&load_config_here()))
        .arg(arg)
        .status();
}

/// Run `program args...` (cargo if `program` is None) with the worktree mapped.
fn run_remapped(program: Option<String>, args: &[String]) {
    let cwd = env::current_dir().unwrap_or_else(|e| die(format!("no current dir: {e}")));
    let cfg = load_config(&cwd);
    let program = program.unwrap_or_else(|| resolve_cargo(&cfg));

    let mut cmd = if env::var_os("CARGO_REMAP_ACTIVE").is_some() {
        // Already inside a remapped command (e.g. a build script calling `cargo`).
        let mut c = Command::new(&program);
        c.args(args);
        c
    } else {
        match backend::Mapping::detect_opt(&cfg, &cwd) {
            Some(mapping) => {
                if env::var_os("CARGO_REMAP_VERBOSE").is_some() {
                    eprintln!(
                        "cargo-remap: mapping {} to {}",
                        mapping.src.display(),
                        mapping.dst.display()
                    );
                }
                backend::command(&cfg, &mapping, &program, args)
            }
            None => {
                // Not in a git repository: nothing to map, just run the command.
                if env::var_os("CARGO_REMAP_VERBOSE").is_some() {
                    eprintln!("cargo-remap: not in a git repository, running without mapping");
                }
                let mut c = Command::new(&program);
                c.args(args);
                c
            }
        }
    };

    if env::var_os("CARGO_REMAP_DRY_RUN").is_some() {
        eprintln!("{cmd:?}");
        return;
    }
    let err = cmd.exec();
    die(format!("failed to exec {:?}: {err}", cmd.get_program()));
}

// --- --help text

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

// --- configuration: .cargo-remap.toml

const CONFIG_NAME: &str = ".cargo-remap.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Absolute path every worktree is mapped to. Defaults to the main worktree
    /// of the git repository.
    pub path: Option<String>,
    /// If set, exported as CARGO_TARGET_DIR so all worktrees share one target dir.
    pub target_dir: Option<String>,
    /// The cargo executable to run. Defaults to the first real cargo in PATH.
    pub cargo: Option<String>,
    /// Where `agents --install` writes the guide, relative to the repo root.
    pub agents_path: Option<String>,
    /// Options for `cargo-remap worktree add`.
    #[serde(default)]
    pub worktree: WorktreeConfig,
}

pub fn find_config(cwd: &Path) -> Option<PathBuf> {
    if let Some(p) = env::var_os("CARGO_REMAP_CONFIG") {
        return Some(PathBuf::from(p));
    }
    for dir in cwd.ancestors() {
        let candidate = dir.join(CONFIG_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
    let candidate = base.join("cargo-remap").join("config.toml");
    candidate.is_file().then_some(candidate)
}

pub fn load_config(cwd: &Path) -> Config {
    let Some(file) = find_config(cwd) else {
        return Config::default();
    };
    let text = fs::read_to_string(&file)
        .unwrap_or_else(|e| die(format!("cannot read {}: {e}", file.display())));
    toml::from_str(&text).unwrap_or_else(|e| die(format!("invalid {}: {e}", file.display())))
}

/// Config for the current directory (defaults if there is none).
pub fn load_config_here() -> Config {
    load_config(&env::current_dir().unwrap_or_default())
}

/// The cargo to run: the configured one, else the first real cargo in PATH.
pub fn resolve_cargo(cfg: &Config) -> String {
    match &cfg.cargo {
        Some(c) => c.clone(),
        None => real_cargo()
            .unwrap_or_else(|| diagnose::no_cargo())
            .display()
            .to_string(),
    }
}

// --- install / uninstall: make `cargo` resolve to this binary
//
// Installing creates a symlink named `cargo` pointing at cargo-remap in a directory that
// comes before the real cargo in PATH. When started under the name `cargo`, cargo-remap
// finds the real cargo by skipping itself in PATH.

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default()
}

/// The first `cargo` in PATH that is not a cargo-remap (this binary, or a link to any
/// copy of it: two installed copies must not mistake each other for the real cargo).
pub fn real_cargo() -> Option<PathBuf> {
    let me = env::current_exe()
        .ok()
        .and_then(|p| fs::canonicalize(p).ok());
    path_dirs().into_iter().map(|d| d.join("cargo")).find(|c| {
        if !c.is_file() {
            return false;
        }
        match fs::canonicalize(c) {
            Ok(real) => {
                real.file_name().is_none_or(|n| n != "cargo-remap") && Some(&real) != me.as_ref()
            }
            Err(_) => false,
        }
    })
}

fn is_ours(link: &Path) -> bool {
    fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
        && fs::read_link(link)
            .ok()
            .and_then(|t| t.file_name().map(|n| n == "cargo-remap"))
            .unwrap_or(false)
}

fn parse_dir(args: &[String]) -> Option<PathBuf> {
    match args {
        [] => None,
        [flag, dir] if flag == "--dir" => Some(PathBuf::from(dir)),
        _ => die("usage: install|uninstall [--dir DIR]"),
    }
}

pub fn install(args: &[String]) {
    let dir = parse_dir(args).unwrap_or_else(|| {
        let home = env::var_os("HOME").unwrap_or_else(|| die("HOME is not set"));
        Path::new(&home).join(".local/bin")
    });
    let real = real_cargo().unwrap_or_else(|| diagnose::no_cargo());
    let exe = env::current_exe()
        .and_then(fs::canonicalize)
        .unwrap_or_else(|e| die(format!("cannot locate own binary: {e}")));
    fs::create_dir_all(&dir)
        .unwrap_or_else(|e| die(format!("cannot create {}: {e}", dir.display())));
    let link = dir.join("cargo");

    if fs::symlink_metadata(&link).is_ok() {
        if !is_ours(&link) {
            die(format!(
                "{} already exists and is not a cargo-remap link",
                link.display()
            ));
        }
        fs::remove_file(&link).unwrap_or_else(|e| die(format!("cannot replace link: {e}")));
    }
    symlink(&exe, &link).unwrap_or_else(|e| die(format!("cannot create {}: {e}", link.display())));
    println!("{} -> {}", link.display(), exe.display());
    println!("real cargo: {}", real.display());

    // Does `cargo` now resolve to the link?
    if !wrapper_active() {
        eprintln!(
            "warning: {} does not come before {} in PATH, so `cargo` still runs the real one.\n\
             Put it first, for the current shell and for new ones (adjust for your shell):\n\
             \x20   export PATH=\"{}:$PATH\"\n\
             \x20   echo 'export PATH=\"{}:$PATH\"' >> ~/.profile\n\
             Shells that do not read ~/.profile (`ssh host command`, `podman exec`, cron) need\n\
             PATH set in the command itself.",
            dir.display(),
            real.parent().unwrap_or(Path::new("?")).display(),
            dir.display(),
            dir.display(),
        );
    }
}

/// Does the first `cargo` in PATH run cargo-remap (a link made by `install`)?
pub fn wrapper_active() -> bool {
    let me = env::current_exe().and_then(fs::canonicalize).ok();
    let first = path_dirs()
        .into_iter()
        .map(|d| d.join("cargo"))
        .find(|c| c.exists());
    me.is_some() && first.as_deref().map(fs::canonicalize).and_then(Result::ok) == me
}

pub fn uninstall(args: &[String]) {
    let links: Vec<PathBuf> = match parse_dir(args) {
        Some(d) => vec![d.join("cargo")],
        None => {
            let mut dirs = path_dirs();
            if let Some(home) = env::var_os("HOME") {
                dirs.push(Path::new(&home).join(".local/bin"));
            }
            dirs.into_iter().map(|d| d.join("cargo")).collect()
        }
    };
    let mut removed = 0;
    let mut seen = Vec::new();
    for link in links {
        if seen.contains(&link) || !is_ours(&link) {
            continue;
        }
        seen.push(link.clone());
        match fs::remove_file(&link) {
            Ok(()) => {
                println!("removed {}", link.display());
                removed += 1;
            }
            Err(e) => die(format!("cannot remove {}: {e}", link.display())),
        }
    }
    if removed == 0 {
        println!("no cargo-remap link named `cargo` found");
    }
}

// --- small helpers

pub fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("cargo-remap: {}", msg.as_ref());
    exit(1);
}

pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME")
    {
        return Path::new(&home).join(rest);
    }
    PathBuf::from(s)
}

pub fn in_path(prog: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|d| d.join(prog).is_file())
}

/// Run git in `cwd`; stdout (trimmed) if it succeeded.
pub fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Only drop git's final newline: a path may legitimately end in spaces.
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.strip_suffix('\n').unwrap_or(&text).to_string())
}

/// The first entry of `git worktree list` is always the main worktree.
/// `-z` keeps paths with newlines in them intact, but needs git 2.36; older gits get
/// the plain form, which cannot represent such paths.
pub fn main_worktree(cwd: &Path) -> Option<PathBuf> {
    let list = |z: bool| {
        let mut args = vec!["worktree", "list", "--porcelain"];
        if z {
            args.push("-z");
        }
        git(cwd, &args)
    };
    let (out, sep) = match list(true) {
        Some(out) => (out, '\0'),
        None => (list(false)?, '\n'),
    };
    let mut fields = out.split(sep).take_while(|f| !f.is_empty());
    let path = fields.next()?.strip_prefix("worktree ")?;
    if fields.any(|f| f == "bare") {
        return None;
    }
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_worktree_of_this_repo_is_absolute() {
        let here = std::env::current_dir().unwrap();
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        if let Some(p) = main_worktree(&here) {
            assert!(p.is_absolute());
        }
    }
}
