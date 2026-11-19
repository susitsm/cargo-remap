//! The `.cargo-remap.toml` configuration.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::diagnose;
use crate::install;
use crate::util::die;
use crate::worktree::WorktreeConfig;

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
        None => install::real_cargo()
            .unwrap_or_else(|| diagnose::no_cargo())
            .display()
            .to_string(),
    }
}
