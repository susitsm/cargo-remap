//! Running a command with the worktree mapped to its fixed path, in a private user +
//! mount namespace. If that is not possible here, [`crate::diagnose`] says why and how
//! to fix it.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::diagnose;
use crate::util::{die, expand_tilde, git, main_worktree};

// Inner script, run in a fresh user + mount namespace where our real uid/gid are
// mapped to themselves (so ownership looks the same as on the host). --keep-caps
// gives the shell the namespace's capabilities so it can bind-mount; they are
// dropped again before the command runs.
// --rbind (not --bind): a plain bind of a directory that has a mount below it, such as
// a btrfs subvolume or a tmpfs, fails with "wrong fs type" in a user namespace.
// A linked worktree's `.git` file points into the main worktree's `.git`, which the
// mapping can hide. Then the real git directory is first mounted at a private spot
// (gsrc -> gdir) and a replacement `.git` file (gfile) is mounted over the worktree's.
// args: src dst rel gsrc gdir gfile cmd...  (the last three empty if not needed)
const SCRIPT: &str = r#"
set -e
src=$1; dst=$2; rel=$3; gsrc=$4; gdir=$5; gfile=$6
shift 6
mkdir -p "$dst"
if [ -n "$gsrc" ]; then mount --rbind "$gsrc" "$gdir"; fi
mount --rbind "$src" "$dst"
if [ -n "$gsrc" ]; then mount --bind "$gfile" "$dst/.git"; fi
cd "$dst/$rel"
exec setpriv --inh-caps=-all --ambient-caps=-all --bounding-set=-all -- "$@"
"#;

/// The uid and gid we run as.
fn owner() -> (u32, u32) {
    let meta = fs::metadata("/proc/self");
    (
        meta.as_ref().map(|m| m.uid()).unwrap_or(0),
        meta.as_ref().map(|m| m.gid()).unwrap_or(0),
    )
}

/// Which directory is mapped where, and for whom.
pub struct Mapping {
    /// Root of the current worktree (or the cwd outside git).
    pub src: PathBuf,
    /// The fixed path it appears at.
    pub dst: PathBuf,
    /// The current directory relative to `src`.
    pub rel: PathBuf,
    pub uid: u32,
    pub gid: u32,
}

impl Mapping {
    /// The mapping for `cwd`, or None when there is nothing to map: not in a git
    /// repository (or git is not installed) and no `path` configured. Plain cargo
    /// commands such as `cargo new` then simply run unmapped.
    pub fn detect_opt(cfg: &Config, cwd: &Path) -> Option<Mapping> {
        if cfg.path.is_none()
            && (!crate::util::in_path("git")
                || git(cwd, &["rev-parse", "--show-toplevel"]).is_none())
        {
            return None;
        }
        Some(Mapping::detect(cfg, cwd))
    }

    pub fn detect(cfg: &Config, cwd: &Path) -> Mapping {
        if cfg.path.is_none() {
            // Without git we cannot tell which worktree this is.
            diagnose::require(&["git"]);
        }
        let (src, rel) = match git(cwd, &["rev-parse", "--show-toplevel"]) {
            Some(top) => {
                let top = PathBuf::from(top);
                let rel = cwd
                    .strip_prefix(&top)
                    .unwrap_or(Path::new(""))
                    .to_path_buf();
                (top, rel)
            }
            None => (cwd.to_path_buf(), PathBuf::new()),
        };
        // Destination: config value, else the main worktree of the repo.
        let dst = match &cfg.path {
            Some(p) => expand_tilde(p),
            None => main_worktree(cwd).unwrap_or_else(|| {
                die(
                    "this directory is not in a git repository (or it is a bare one).\n\
                     Run cargo-remap inside a git worktree, or set `path = \"/abs/path\"` in\n\
                     .cargo-remap.toml to say where to map the current directory",
                )
            }),
        };
        if !dst.is_absolute() {
            die(format!("mapped path must be absolute: {}", dst.display()));
        }
        let (uid, gid) = owner();
        Mapping {
            src,
            dst,
            rel,
            uid,
            gid,
        }
    }

    /// A mapping of `dir` onto itself, for checking that namespaces work outside a repo.
    fn onto_itself(dir: &Path) -> Mapping {
        let (uid, gid) = owner();
        Mapping {
            src: dir.to_path_buf(),
            dst: dir.to_path_buf(),
            rel: PathBuf::new(),
            uid,
            gid,
        }
    }

    /// Already at the mapped path (e.g. the main worktree): nothing to do.
    fn is_identity(&self) -> bool {
        fs::canonicalize(&self.src).ok() == fs::canonicalize(&self.dst).ok()
    }

    /// The working directory inside the mapping, relative to the mapped path.
    fn rel_or_dot(&self) -> &Path {
        if self.rel.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &self.rel
        }
    }
}

/// Environment the remapped command gets on top of ours.
fn extra_env(cfg: &Config) -> Vec<(String, String)> {
    let mut env = vec![("CARGO_REMAP_ACTIVE".to_string(), "1".to_string())];
    if let Some(t) = &cfg.target_dir {
        env.push((
            "CARGO_TARGET_DIR".to_string(),
            expand_tilde(t).display().to_string(),
        ));
    }
    env
}

/// The command that runs `program args...` with the mapping in place.
pub fn command(cfg: &Config, m: &Mapping, program: &str, args: &[String]) -> Command {
    let env = extra_env(cfg);
    let mut cmd = if m.is_identity() {
        let mut c = Command::new(program);
        c.args(args);
        c
    } else {
        diagnose::require(&["unshare", "setpriv", "mount", "sh"]);
        check_namespace(m);
        namespace_cmd(m, &m.dst, m.rel_or_dot(), program, args)
    };
    for (k, v) in &env {
        cmd.env(k, v);
    }
    cmd
}

fn unshare_base(m: &Mapping) -> Command {
    let mut cmd = Command::new("unshare");
    cmd.args([
        "--user",
        "--keep-caps",
        "--mount",
        "--propagation",
        "private",
    ])
    .arg(format!("--map-user={}", m.uid))
    .arg(format!("--map-group={}", m.gid));
    cmd
}

/// Everything a run needs, checked up front: the programs, and a working namespace.
/// Returns the mapping for the current directory.
pub fn preflight(cfg: &Config, cwd: &Path) -> Mapping {
    diagnose::require(&["unshare", "setpriv", "mount", "sh"]);
    let m = Mapping::detect(cfg, cwd);
    check_namespace(&m);
    m
}

/// The same checks without needing a git repository (for `doctor` run anywhere).
pub fn preflight_anywhere(cwd: &Path) {
    diagnose::require(&["unshare", "setpriv", "mount", "sh"]);
    check_namespace(&Mapping::onto_itself(cwd));
}

/// Do the exact steps of a real run (new namespace, bind mount, drop capabilities)
/// with a no-op command, mapping the worktree onto itself, and explain the failure if
/// there is one.
fn check_namespace(m: &Mapping) {
    let out = namespace_cmd(m, &m.src, Path::new("."), "true", &[])
        .stdin(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => diagnose::namespace_failed(&String::from_utf8_lossy(&o.stderr)),
        Err(e) => diagnose::namespace_failed(&e.to_string()),
    }
}

/// `program args...` with `m.src` mounted at `dst`, started in `rel` below it.
fn namespace_cmd(m: &Mapping, dst: &Path, rel: &Path, program: &str, args: &[String]) -> Command {
    let mut cmd = unshare_base(m);
    let (gsrc, gdir, gfile) = git_overlay(m, dst).unwrap_or_default();
    cmd.args(["sh", "-c", SCRIPT, "sh"])
        .arg(&m.src)
        .arg(dst)
        .arg(rel)
        .arg(gsrc)
        .arg(gdir)
        .arg(gfile)
        .arg(program)
        .args(args);
    cmd
}

/// For a linked worktree whose main `.git` directory would be hidden by mapping onto
/// `dst`: (the real git directory, a private path to mount it at, a replacement `.git`
/// file pointing there). Without this, git fails inside the mapped worktree with
/// "gitfile does not point to a valid repository".
fn git_overlay(m: &Mapping, dst: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    if dst == m.src || !m.src.join(".git").is_file() {
        return None;
    }
    let abs = |arg: &str| {
        let p = PathBuf::from(crate::util::git(&m.src, &["rev-parse", arg])?);
        Some(if p.is_absolute() { p } else { m.src.join(p) })
    };
    let common = fs::canonicalize(abs("--git-common-dir")?).ok()?;
    let gitdir = fs::canonicalize(abs("--git-dir")?).ok()?;
    if !common.starts_with(dst) {
        return None;
    }
    let rel = gitdir.strip_prefix(&common).ok()?;
    let base = std::env::temp_dir().join(format!("cargo-remap-{}", m.uid));
    let mount = base.join("git");
    fs::create_dir_all(&mount).ok()?;
    let name = m.src.to_string_lossy().replace('/', "_");
    let file = base.join(format!("gitfile{name}"));
    fs::write(&file, format!("gitdir: {}\n", mount.join(rel).display())).ok()?;
    Some((common, mount, file))
}
