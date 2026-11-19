//! Telling the user what is wrong, and how to fix it, when remapping cannot work.
//!
//! cargo-remap needs a few programs and the kernel's unprivileged user namespaces.
//! Every way that can fail ends in [`die`] with a message that names the cause and the
//! fix, instead of a bare `unshare: ...` or `mount: permission denied`.

use std::fs;
use std::path::Path;

use crate::util::{die, in_path};

/// Which package provides a program, per distro family (we do not try to detect the
/// distro, so every family gets its own line).
#[derive(Clone, Copy)]
enum Family {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Apk,
}

fn package(prog: &str, family: Family) -> &'static str {
    match (prog, family) {
        ("git", _) => "git",
        ("cp", _) => "coreutils",
        // Alpine splits util-linux into many packages.
        ("unshare", Family::Apk) => "util-linux-misc",
        ("setpriv", Family::Apk) => "setpriv",
        ("mount", Family::Apk) => "mount",
        // Debian and Ubuntu: `mount` was a package of its own until recent releases.
        ("mount", Family::Apt) => "mount",
        ("unshare" | "setpriv" | "mount", _) => "util-linux",
        ("sh", _) => "dash",
        _ => "",
    }
}

/// What provides a program, for the "is in the package" list.
fn package_summary(prog: &str) -> String {
    match prog {
        "git" => "`git`".into(),
        "cp" => "`coreutils`".into(),
        "sh" => "`dash` or `bash`".into(),
        _ => format!("`util-linux` (Alpine: `{}`)", package(prog, Family::Apk)),
    }
}

const UTIL_LINUX: &[&str] = &["unshare", "setpriv", "mount"];

/// Is the first `prog` in PATH a link to BusyBox?
fn is_busybox(prog: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|c| c.is_file())
        .and_then(|c| fs::canonicalize(c).ok())
        .is_some_and(|c| c.file_name().is_some_and(|n| n == "busybox"))
}

/// Exit with one message listing every program of `needed` that is not in PATH.
pub fn require(needed: &[&str]) {
    // A BusyBox applet counts as missing: it lacks the options we need, and reporting it
    // here puts every problem in one message instead of one per attempt.
    let missing: Vec<&str> = needed
        .iter()
        .copied()
        .filter(|p| !in_path(p) || (UTIL_LINUX.contains(p) && is_busybox(p)))
        .collect();
    if missing.is_empty() {
        return;
    }
    let lines: String = missing
        .iter()
        .map(|p| {
            let why = if in_path(p) {
                " (the BusyBox one is not enough)"
            } else {
                ""
            };
            format!("    {p:<8} is in the package {}{why}\n", package_summary(p))
        })
        .collect();
    die(format!(
        "missing programs: {}\n\n{lines}\nInstall them with your package manager:\n{}",
        missing.join(", "),
        install_commands(&missing)
    ));
}

/// One install command per distro family.
fn install_commands(programs: &[&str]) -> String {
    let line = |family: Family| {
        let mut pkgs: Vec<&str> = Vec::new();
        for p in programs {
            let pkg = package(p, family);
            if !pkg.is_empty() && !pkgs.contains(&pkg) {
                pkgs.push(pkg);
            }
        }
        pkgs.join(" ")
    };
    format!(
        "    Debian, Ubuntu:    sudo apt install {}\n\
         \x20   Fedora, RHEL:      sudo dnf install {}\n\
         \x20   Arch:              sudo pacman -S {}\n\
         \x20   openSUSE:          sudo zypper install {}\n\
         \x20   Alpine:            sudo apk add {}",
        line(Family::Apt),
        line(Family::Dnf),
        line(Family::Pacman),
        line(Family::Zypper),
        line(Family::Apk),
    )
}

/// The `cargo` to run was not found.
pub fn no_cargo() -> ! {
    die("cargo was not found in PATH.\n\n\
         Install Rust with rustup (https://rustup.rs):\n\
         \x20   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n\
         then open a new shell so that ~/.cargo/bin is in PATH. If cargo is installed\n\
         elsewhere, add its directory to PATH or set `cargo = \"/path/to/cargo\"` in\n\
         .cargo-remap.toml.")
}

/// Where `prog` lives (the first match in PATH, with symlinks resolved).
fn program_path(prog: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|c| c.is_file())
        .and_then(|c| fs::canonicalize(c).ok())
        .map(|c| c.display().to_string())
}

fn read(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists()
}

/// The namespace probe failed with `stderr`: say why, and what to do.
pub fn namespace_failed(stderr: &str) -> ! {
    let err = stderr.trim();
    let lower = err.to_lowercase();
    let underlying = if err.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nThe underlying error was:\n    {}",
            err.replace('\n', "\n    ")
        )
    };

    if lower.contains("unrecognized option") || lower.contains("busybox") {
        die(format!(
            "`unshare` or `setpriv` here is the BusyBox version, which lacks options we need.\n\n\
             Install util-linux and make sure its programs come first in PATH:\n{}{underlying}",
            install_commands(&["unshare", "setpriv", "mount"])
        ));
    }

    // Inside a container the host's settings are visible but not ours to change, so the
    // container advice comes first.
    let permission = lower.contains("operation not permitted")
        || lower.contains("permission denied")
        || lower.contains("must be superuser")
        || lower.contains("eperm");
    if in_container() && permission {
        die(format!(
            "Creating a namespace or mounting is refused inside this container.\n\n\
             Easiest fix: use rootless podman. It works with its default\n\
             settings, as root or as a normal user inside the container:\n\
             \x20   podman run --rm -it -v \"$PWD\":/work -w /work IMAGE ...\n\
             (add :z to the -v on SELinux hosts such as Fedora). Or run cargo-remap outside\n\
             the container.\n\n\
             Details: \"Containers\" in the README.{underlying}"
        ));
    }

    if read("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").as_deref() == Some("1") {
        let unshare = program_path("unshare").unwrap_or_else(|| "/usr/bin/unshare".into());
        die(format!(
            "AppArmor blocks unprivileged user namespaces on this system (Ubuntu 23.10+\n\
             and some other distros do this by default), and cargo-remap needs one.\n\n\
             Allow it for the `unshare` program only (other programs stay restricted).\n\
             Run this once; it also survives reboots:\n\n\
             sudo tee /etc/apparmor.d/cargo-remap-unshare >/dev/null <<'EOF'\n\
abi <abi/4.0>,\n\
include <tunables/global>\n\
\n\
profile cargo-remap-unshare {unshare} flags=(unconfined) {{\n\
  userns,\n\
  include if exists <local/cargo-remap-unshare>\n\
}}\n\
EOF\n\
             sudo apparmor_parser -r /etc/apparmor.d/cargo-remap-unshare\n\n\
             Then run the same command again. Anyone who can run {unshare} can then create\n\
             user namespaces. To undo: sudo apparmor_parser -R /etc/apparmor.d/cargo-remap-unshare\n\
             && sudo rm /etc/apparmor.d/cargo-remap-unshare\n\n\
             If that fails because your AppArmor is older than 4.0 (Ubuntu 23.10), the only\n\
             switch is the system-wide one, which allows user namespaces for every program:\n\
             \x20   sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0{underlying}"
        ));
    }

    if read("/proc/sys/user/max_user_namespaces").as_deref() == Some("0") {
        die(format!(
            "User namespaces are disabled in this kernel (user.max_user_namespaces is 0).\n\n\
             To enable them, run:\n\
             \x20   sudo sysctl -w user.max_user_namespaces=15000\n\
             and to keep the setting after a reboot:\n\
             \x20   echo 'user.max_user_namespaces=15000' | sudo tee /etc/sysctl.d/60-cargo-remap.conf{underlying}"
        ));
    }

    if read("/proc/sys/kernel/unprivileged_userns_clone").as_deref() == Some("0") {
        die(format!(
            "Unprivileged user namespaces are disabled in this kernel\n\
             (kernel.unprivileged_userns_clone is 0).\n\n\
             To enable them, run:\n\
             \x20   sudo sysctl -w kernel.unprivileged_userns_clone=1\n\
             and to keep the setting after a reboot:\n\
             \x20   echo 'kernel.unprivileged_userns_clone=1' | sudo tee /etc/sysctl.d/60-cargo-remap.conf{underlying}"
        ));
    }

    let state = |p: &str| read(p).unwrap_or_else(|| "n/a".into());
    die(format!(
        "could not set up the mount namespace, and the cause is not one we recognise.\n\n\
         Things to check:\n\
         \x20   - if the error below names a directory, the mount of that directory failed:\n\
         \x20     try running from another directory (a project directory, not $HOME)\n\
         \x20   - unprivileged user namespaces must be allowed (SELinux, a hardened kernel or\n\
         \x20     a security policy can forbid them)\n\
         \x20   - `unshare --user --map-root-user true` must work in a shell\n\n\
         Kernel settings here: user.max_user_namespaces={}, kernel.unprivileged_userns_clone={},\n\
         kernel.apparmor_restrict_unprivileged_userns={}{underlying}",
        state("/proc/sys/user/max_user_namespaces"),
        state("/proc/sys/kernel/unprivileged_userns_clone"),
        state("/proc/sys/kernel/apparmor_restrict_unprivileged_userns"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_commands_cover_every_family() {
        let text = install_commands(&["git", "setpriv"]);
        for needle in [
            "apt install git util-linux",
            "dnf install git util-linux",
            "pacman -S git util-linux",
            "zypper install git util-linux",
            "apk add git setpriv",
        ] {
            assert!(text.contains(needle), "{needle} missing in {text}");
        }
    }

    #[test]
    fn alpine_gets_its_split_packages_and_debian_the_mount_package() {
        let text = install_commands(&["unshare", "setpriv", "mount"]);
        assert!(
            text.contains("apk add util-linux-misc setpriv mount"),
            "{text}"
        );
        assert!(text.contains("apt install util-linux mount"), "{text}");
        assert!(!install_commands(&["setpriv"]).contains("mount"));
    }

    #[test]
    fn every_required_program_has_a_package_on_every_family() {
        for prog in ["git", "unshare", "setpriv", "mount", "cp", "sh"] {
            for f in [
                Family::Apt,
                Family::Dnf,
                Family::Pacman,
                Family::Zypper,
                Family::Apk,
            ] {
                assert!(!package(prog, f).is_empty(), "{prog}");
            }
        }
    }
}
