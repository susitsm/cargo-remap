//! Small helpers shared by the modules.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

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
    use std::process::Command;

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
