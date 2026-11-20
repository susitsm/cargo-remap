//! Turning the command line into an [`Action`].
//!
//! Started as `cargo` (a symlink made by `install`) everything is for cargo,
//! except `cargo remap <command>`. Started as `cargo-remap` or `cargo remap`, only
//! our own commands exist and nothing is ever passed to cargo implicitly.

use std::path::Path;

use crate::die;

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Print our help.
    Help,
    /// `cargo --help` / `cargo help` as a stand-in: a note, then cargo's own help.
    CargoHelp(String),
    Install(Vec<String>),
    Uninstall(Vec<String>),
    Worktree(Vec<String>),
    Agents(Vec<String>),
    /// Check that everything needed is here and say so.
    Doctor,
    /// Run a program remapped; `None` means cargo.
    Run {
        program: Option<String>,
        args: Vec<String>,
    },
}

/// Were we started through a symlink named `cargo`?
pub fn invoked_as_cargo() -> bool {
    std::env::args()
        .next()
        .is_some_and(|a| Path::new(&a).file_name().is_some_and(|n| n == "cargo"))
}

pub fn parse(as_cargo: bool, mut args: Vec<String>) -> Action {
    // `cargo remap ...` / `cargo-remap remap ...`: the subcommand name is optional.
    let explicit = args.first().map(String::as_str) == Some("remap");
    if explicit {
        args.remove(0);
    }
    if as_cargo && !explicit {
        parse_as_cargo(args)
    } else {
        parse_own(args)
    }
}

fn is_help(args: &[String]) -> bool {
    matches!(args, [a] if matches!(a.as_str(), "--help" | "-h" | "help"))
}

fn parse_as_cargo(args: Vec<String>) -> Action {
    if is_help(&args) {
        Action::CargoHelp(args[0].clone())
    } else {
        Action::Run {
            program: None,
            args,
        }
    }
}

fn parse_own(mut args: Vec<String>) -> Action {
    if args.is_empty() || is_help(&args) {
        return Action::Help;
    }
    let command = args.remove(0);
    match command.as_str() {
        "install" => Action::Install(args),
        "uninstall" => Action::Uninstall(args),
        "worktree" => Action::Worktree(args),
        "agents" => Action::Agents(args),
        "doctor" => Action::Doctor,
        "cargo" => Action::Run {
            program: None,
            args,
        },
        "exec" => {
            if args.first().map(String::as_str) == Some("--") {
                args.remove(0);
            }
            if args.is_empty() {
                die("usage: cargo-remap exec <command> [args...]");
            }
            let program = args.remove(0);
            if program.is_empty() {
                die("exec: empty command");
            }
            Action::Run {
                program: Some(program),
                args,
            }
        }
        other => die(format!(
            "unknown command `{other}` (see --help). Commands are never passed to cargo; \
             to run cargo remapped use `cargo-remap cargo {other} ...`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn as_cargo_forwards_everything() {
        assert_eq!(
            parse(true, v(&["build", "--release"])),
            Action::Run {
                program: None,
                args: v(&["build", "--release"])
            }
        );
        assert_eq!(
            parse(true, v(&[])),
            Action::Run {
                program: None,
                args: v(&[])
            }
        );
        assert_eq!(
            parse(true, v(&["worktree"])),
            Action::Run {
                program: None,
                args: v(&["worktree"])
            }
        );
    }

    #[test]
    fn as_cargo_help_gets_a_note() {
        for h in ["--help", "-h", "help"] {
            assert_eq!(parse(true, v(&[h])), Action::CargoHelp(h.to_string()));
        }
        // Subcommand help goes to cargo untouched.
        assert!(matches!(
            parse(true, v(&["help", "build"])),
            Action::Run { .. }
        ));
        assert!(matches!(
            parse(true, v(&["build", "--help"])),
            Action::Run { .. }
        ));
    }

    #[test]
    fn remap_prefix_selects_our_commands() {
        assert_eq!(parse(true, v(&["remap"])), Action::Help);
        assert_eq!(parse(true, v(&["remap", "--help"])), Action::Help);
        assert_eq!(
            parse(true, v(&["remap", "worktree", "add", "x"])),
            Action::Worktree(v(&["add", "x"]))
        );
        assert_eq!(
            parse(false, v(&["remap", "agents"])),
            Action::Agents(v(&[]))
        );
    }

    #[test]
    fn own_commands() {
        assert_eq!(parse(false, v(&[])), Action::Help);
        assert_eq!(parse(false, v(&["help"])), Action::Help);
        assert_eq!(
            parse(false, v(&["install", "--dir", "d"])),
            Action::Install(v(&["--dir", "d"]))
        );
        assert_eq!(
            parse(false, v(&["cargo", "build"])),
            Action::Run {
                program: None,
                args: v(&["build"])
            }
        );
        assert_eq!(
            parse(false, v(&["exec", "--", "ls", "-l"])),
            Action::Run {
                program: Some("ls".into()),
                args: v(&["-l"])
            }
        );
    }
}
