#![doc = include_str!("../README.md")]

use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

mod agents;
mod backend;
mod cli;
mod config;
mod diagnose;
mod help;
mod install;
mod util;
mod worktree;

use cli::Action;
use config::{load_config, load_config_here, resolve_cargo};
use util::die;

fn main() {
    let args = env::args().skip(1).collect();
    match cli::parse(cli::invoked_as_cargo(), args) {
        Action::Help => print!("{}", help::HELP),
        Action::CargoHelp(arg) => cargo_help(&arg),
        Action::Install(args) => install::install(&args),
        Action::Uninstall(args) => install::uninstall(&args),
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
    let in_repo = util::git(&cwd, &["rev-parse", "--show-toplevel"]).is_some();
    if in_repo || cfg.path.is_some() {
        let m = backend::preflight(&cfg, &cwd);
        println!("ok: a mount namespace can be created here");
        println!(
            "ok: this worktree ({}) is mapped to {}",
            m.src.display(),
            m.dst.display()
        );
        if util::git(&m.src, &["rev-parse", "--verify", "-q", "HEAD"]).is_none() {
            println!(
                "note: this repository has no commits yet, and `worktree add` needs one:\n\
                 \x20     git add -A && git commit -m \"first commit\""
            );
        }
        if m.src.join("Cargo.toml").exists()
            && util::git(&m.src, &["check-ignore", "-q", "target"]).is_none()
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
    let probe_dir =
        util::git(&cwd, &["rev-parse", "--show-toplevel"]).map_or(cwd.clone(), Into::into);
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
    if install::wrapper_active() {
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
