//! `cargo-remap agents`: print, or install into the current repo, a guide that tells
//! coding agents how to run commands and create worktrees under cargo-remap.
//!
//! The guide is a skill file (default `.agents/skills/cargo-remap/SKILL.md`, any path
//! via `--path` or the `agents_path` config key), readable by any agent. The
//! project's AGENTS.md only gets a short managed block that points to it, so
//! `agents --install` can refresh the skill without touching the rest of AGENTS.md.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{die, git};

const GUIDE: &str = include_str!("agents-skill.md");
pub const DEFAULT_PATH: &str = ".agents/skills/cargo-remap/SKILL.md";
const BEGIN: &str = "<!-- cargo-remap:begin -->";
const END: &str = "<!-- cargo-remap:end -->";

fn block(skill: &str) -> String {
    format!(
        "{BEGIN}\n\
## Building and worktrees (cargo-remap)\n\n\
This repo maps every git worktree to one fixed path with cargo-remap. Read\n\
`{skill}` before running cargo or any path-dependent command (debuggers,\n\
formatters, linters, scripts, test runners), and before creating worktrees. In short:\n\n\
- Run cargo as `cargo remap cargo <args>` (or plain `cargo` if `install` was done).\n\
- Run other path-sensitive commands as `cargo remap exec <command>`; run git normally.\n\
- Create worktrees with `cargo remap worktree add <path> [-b <branch>]`, not `git worktree add`.\n\
- Paths in output under the mapped path are files of your current worktree.\n\
{END}\n"
    )
}

fn write_if_changed(path: &Path, content: &str) -> &'static str {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return "unchanged";
    }
    let existed = path.exists();
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)
            .unwrap_or_else(|e| die(format!("cannot create {}: {e}", p.display())));
    }
    fs::write(path, content)
        .unwrap_or_else(|e| die(format!("cannot write {}: {e}", path.display())));
    if existed { "updated" } else { "created" }
}

/// Put the managed block into AGENTS.md: replace the old block, else append.
fn update_agents_md(root: &Path, skill: &str) -> (PathBuf, &'static str) {
    let path = ["AGENTS.md", "agents.md"]
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.exists())
        .unwrap_or_else(|| root.join("AGENTS.md"));
    let old = fs::read_to_string(&path).ok();
    let new = match &old {
        None => format!("# AGENTS.md\n\n{}", block(skill)),
        Some(text) => match (text.find(BEGIN), text.find(END)) {
            (Some(a), Some(b)) if a < b => {
                let end = b + END.len();
                let end = if text[end..].starts_with('\n') {
                    end + 1
                } else {
                    end
                };
                format!("{}{}{}", &text[..a], block(skill), &text[end..])
            }
            _ => {
                let sep = if text.is_empty() || text.ends_with("\n\n") {
                    ""
                } else if text.ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                format!("{text}{sep}{}", block(skill))
            }
        },
    };
    let status = write_if_changed(&path, &new);
    (path, status)
}

pub fn run(args: &[String], config_path: Option<&str>) {
    let mut install = false;
    let mut path = config_path.map(String::from);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--install" => install = true,
            "--path" => {
                path = Some(
                    it.next()
                        .unwrap_or_else(|| die("--path needs a value"))
                        .clone(),
                );
            }
            s if s.starts_with("--path=") => path = Some(s["--path=".len()..].to_string()),
            other => die(format!(
                "unknown argument: {other} (usage: agents [--install] [--path FILE])"
            )),
        }
    }
    if !install {
        print!("{GUIDE}");
        return;
    }

    let cwd = env::current_dir().unwrap_or_else(|e| die(format!("no current dir: {e}")));
    let root = git(&cwd, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .unwrap_or_else(|| die("not in a git repository"));
    // Relative paths are relative to the repo root; a directory gets SKILL.md added.
    let raw = path.unwrap_or_else(|| DEFAULT_PATH.to_string());
    let mut skill = root.join(&raw);
    if raw.ends_with('/') || skill.is_dir() {
        skill.push("SKILL.md");
    }
    // What AGENTS.md says: relative to the repo root when possible.
    let reference = skill
        .strip_prefix(&root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| skill.display().to_string());

    println!("{}: {}", write_if_changed(&skill, GUIDE), skill.display());
    let (path, status) = update_agents_md(&root, &reference);
    println!("{status}: {}", path.display());
}
