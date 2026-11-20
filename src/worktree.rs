//! `cargo-remap worktree add`: create a git worktree, seed it with copies of the
//! build state of the current worktree, and fix mtimes so cargo sees it as fresh.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio, exit};

use serde::Deserialize;

use crate::config::{find_config, load_config};
use crate::util::{die, git};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reflink {
    Always,
    Auto,
    Never,
}

impl Reflink {
    /// The `cp` option. A full copy passes none, because BusyBox's cp has no `--reflink`.
    fn flag(self) -> Option<&'static str> {
        match self {
            Reflink::Always => Some("--reflink=always"),
            Reflink::Auto => Some("--reflink=auto"),
            Reflink::Never => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WorktreeConfig {
    /// Copy `target/` from the current worktree.
    pub copy_target: bool,
    /// Copy everything git ignores (except `target/`, which has its own switch).
    pub copy_gitignored: bool,
    /// Extra paths (relative to the worktree root) to copy.
    pub copy_files: Vec<String>,
    /// How to copy: "auto" (default: copy on write if the filesystem supports it, else a
    /// full copy), "always" (copy on write; if unsupported, skip the copy with a
    /// warning) or "never" (always a full copy).
    pub reflink: Reflink,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            copy_target: true,
            copy_gitignored: true,
            copy_files: Vec::new(),
            reflink: Reflink::Auto,
        }
    }
}

pub fn run(args: &[String]) {
    if args.first().map(String::as_str) == Some("add") {
        add(&args[1..]);
    } else {
        // Everything else is plain `git worktree ...`.
        let status = Command::new("git")
            .arg("worktree")
            .args(args)
            .status()
            .unwrap_or_else(|e| die(format!("cannot run git: {e}")));
        exit(status.code().unwrap_or(1));
    }
}

/// The first positional argument of `git worktree add`.
fn parse_new_path(args: &[String]) -> Option<&String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-b" | "-B" | "--reason" => {
                it.next();
            }
            s if s.starts_with('-') => {}
            _ => return Some(a),
        }
    }
    None
}

fn add(args: &[String]) {
    crate::diagnose::require(&["git", "cp"]);
    let cwd = env::current_dir().unwrap_or_else(|e| die(format!("no current dir: {e}")));
    let src = git(&cwd, &["rev-parse", "--show-toplevel"])
        .unwrap_or_else(|| die("not in a git repository"));
    let src = fs::canonicalize(src).unwrap_or_else(|e| die(format!("bad source path: {e}")));
    let cfg = load_config(&cwd);
    let wt = &cfg.worktree;
    let new_path = parse_new_path(args).unwrap_or_else(|| die("missing worktree path"));
    if git(&src, &["rev-parse", "--verify", "-q", "HEAD"]).is_none() {
        die(
            "this repository has no commits yet, and a worktree needs one to start from.\n\
             Commit first, for example:\n\
             \x20   git add -A && git commit -m \"first commit\"",
        );
    }
    // Fail now, with an explanation, rather than at the first build in the new worktree.
    crate::backend::preflight(&cfg, &cwd);

    let status = Command::new("git")
        .args(["worktree", "add"])
        .args(args)
        .status()
        .unwrap_or_else(|e| die(format!("cannot run git: {e}")));
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }
    let dst = fs::canonicalize(cwd.join(new_path))
        .unwrap_or_else(|e| die(format!("cannot find new worktree: {e}")));

    if wt.copy_target {
        if cfg.target_dir.is_some() {
            eprintln!("cargo-remap: `target_dir` is set (shared), not copying target/");
        } else {
            copy_entry(&src, &dst, Path::new("target"), wt.reflink);
        }
    }
    if wt.copy_gitignored {
        let list = git(
            &src,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ],
        )
        .unwrap_or_default();
        for entry in list.split('\0').filter(|e| !e.is_empty()) {
            let entry = entry.trim_end_matches('/');
            // `target` has its own switch; never copy a tree into itself.
            if entry == "target" || dst.starts_with(src.join(entry)) {
                continue;
            }
            copy_entry(&src, &dst, Path::new(entry), wt.reflink);
        }
    }
    // Carry the cargo-remap config over if git does not (untracked or ignored).
    if let Some(cfg_file) = find_config(&cwd).and_then(|f| fs::canonicalize(f).ok())
        && let Ok(rel) = cfg_file.strip_prefix(&src)
        && git(
            &src,
            &["ls-files", "--error-unmatch", "--", &rel.to_string_lossy()],
        )
        .is_none()
    {
        copy_entry(&src, &dst, rel, Reflink::Auto);
    }
    for f in &wt.copy_files {
        copy_entry(&src, &dst, Path::new(f), wt.reflink);
    }

    sync_mtimes(&src, &dst);
}

/// `cp -a` of `from` to `to` (merging into an existing directory), with cp's complaints
/// captured. Err is cp's first line of complaint.
fn cp(from: &Path, to: &Path, reflink: Reflink) -> Result<(), String> {
    let out = Command::new("cp")
        .arg("-a")
        .args(reflink.flag())
        .args(["-T", "--"])
        .arg(from)
        .arg(to)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run cp: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err.lines().next().unwrap_or("cp failed").to_string())
}

/// Can `cp --reflink=always` copy a file inside `dir`? (Leaves nothing behind.)
pub fn reflink_works(dir: &Path) -> bool {
    let a = dir.join(".cargo-remap-probe-a");
    let b = dir.join(".cargo-remap-probe-b");
    let ok = fs::write(&a, b"x").is_ok() && cp(&a, &b, Reflink::Always).is_ok();
    let _ = fs::remove_file(&a);
    let _ = fs::remove_file(&b);
    ok
}

/// Human-readable size of a file tree (`du -sh`), or "?".
fn size_of(path: &Path) -> String {
    Command::new("du")
        .arg("-sh")
        .arg(path)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(String::from))
        .unwrap_or_else(|| "?".into())
}

/// Copy `rel` from one worktree to the other, and say how it went in one or two lines.
/// A copy on write copy is tried first unless `reflink` is "never". Where the
/// filesystem has none, "auto" copies in full and "always" skips the copy.
fn copy_entry(src_root: &Path, dst_root: &Path, rel: &Path, reflink: Reflink) {
    let from = src_root.join(rel);
    let to = dst_root.join(rel);
    let name = rel.display();
    if from.symlink_metadata().is_err() {
        eprintln!("cargo-remap: skipping missing {}", from.display());
        return;
    }
    if let Some(parent) = to.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Remember whether we are creating `to`, so a failed copy can be cleaned up
    // without touching files that were already there.
    let fresh = to.symlink_metadata().is_err();
    let cleanup = || {
        if fresh {
            let _ = fs::remove_dir_all(&to).or_else(|_| fs::remove_file(&to));
        }
    };

    let never = matches!(reflink, Reflink::Never);
    match cp(
        &from,
        &to,
        if never {
            Reflink::Never
        } else {
            Reflink::Always
        },
    ) {
        Ok(()) => {
            let how = if never { "full copy" } else { "copy on write" };
            eprintln!("cargo-remap: copied {name} ({how})");
            return;
        }
        Err(e) if never => {
            cleanup();
            eprintln!("cargo-remap: warning: could not copy {name}: {e}");
            return;
        }
        Err(_) => cleanup(),
    }
    // No copy on write copies here (or cp refused for another reason).
    if matches!(reflink, Reflink::Always) {
        eprintln!(
            "cargo-remap: warning: could not copy {name} with copy on write: this filesystem\n\
             \x20   does not support it. The new worktree will build {name} from scratch.\n\
             \x20   To allow a full copy instead, put `[worktree]` and `reflink = \"auto\"`\n\
             \x20   in .cargo-remap.toml."
        );
        return;
    }
    eprintln!(
        "cargo-remap: this filesystem has no copy on write copies, so {name} ({}) is copied in full",
        size_of(&from)
    );
    match cp(&from, &to, Reflink::Never) {
        Ok(()) => eprintln!("cargo-remap: copied {name} (full copy)"),
        Err(e) => {
            cleanup();
            eprintln!("cargo-remap: warning: could not copy {name}: {e}");
        }
    }
}

/// path -> blob hash for tracked regular files.
fn tracked(root: &Path) -> HashMap<String, String> {
    let out = git(root, &["ls-files", "-s", "-z"]).unwrap_or_default();
    let mut map = HashMap::new();
    for rec in out.split('\0').filter(|r| !r.is_empty()) {
        let Some((meta, path)) = rec.split_once('\t') else {
            continue;
        };
        let mut f = meta.split(' ');
        let (mode, hash, stage) = (f.next(), f.next(), f.next());
        if stage == Some("0") && mode != Some("120000") && mode != Some("160000") {
            map.insert(path.to_string(), hash.unwrap_or_default().to_string());
        }
    }
    map
}

/// A fresh checkout stamps every file with "now", which makes cargo rebuild
/// everything. For files whose content is identical to the source worktree
/// (committed content, and the source file is unmodified), copy the source mtime.
/// Files that differ keep their new mtime so cargo rebuilds them.
fn sync_mtimes(src: &Path, dst: &Path) {
    let src_files = tracked(src);
    let dirty: HashSet<String> = git(src, &["ls-files", "-m", "-z"])
        .unwrap_or_default()
        .split('\0')
        .map(String::from)
        .collect();
    let mut synced = 0usize;
    for (path, hash) in tracked(dst) {
        if src_files.get(&path) != Some(&hash) || dirty.contains(&path) {
            continue;
        }
        let Ok(mtime) = fs::metadata(src.join(&path)).and_then(|m| m.modified()) else {
            continue;
        };
        let target: PathBuf = dst.join(&path);
        let Ok(file) = OpenOptions::new().write(true).open(&target) else {
            continue;
        };
        if file.set_modified(mtime).is_ok() {
            synced += 1;
        }
    }
    eprintln!("cargo-remap: restored mtimes of {synced} unchanged files");
}
