//! `install` / `uninstall`: make `cargo` resolve to this binary.
//!
//! Installing creates a symlink named `cargo` pointing at cargo-remap in a
//! directory that comes before the real cargo in PATH. When started under the
//! name `cargo`, cargo-remap finds the real cargo by skipping itself in PATH.

use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::util::die;

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
    let real = real_cargo().unwrap_or_else(|| crate::diagnose::no_cargo());
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
