//! Exercises the remote `install-skill.sh` end-to-end without touching the
//! network, mirroring the style of `install_script.rs`.
//!
//! The script's source host (`AUTOMEDON_SKILL_BASE_URL`) is an override point, so
//! the test stands up a fake repo archive as a local file and points the script
//! at it with a `file://` URL. The installed skill is plain files, so "did it
//! land in the skills dir" is observable without a clone or the network.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway directory under the system temp dir, removed on Drop. Mirrors the
/// isolation helper in the other integration tests.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-it-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn install_skill() -> PathBuf {
    repo_root().join("install-skill.sh")
}

/// Lay down a fake source archive (`<dir>/<ref>.tar.gz`) shaped like a GitHub
/// repo tarball: a single `automedon-<ref>/` top-level directory holding a
/// `skills/automedon/` skill. `with_skill = false` omits the skill directory to
/// simulate an archive the installer must reject.
fn fake_archive(release_dir: &Path, git_ref: &str, with_skill: bool) {
    let stage = release_dir.join("stage");
    let top = stage.join(format!("automedon-{git_ref}"));
    if with_skill {
        let skill = top.join("skills").join("automedon");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: automedon\n---\n").unwrap();
        std::fs::write(skill.join("README.md"), "# Operating the Automedon engine\n").unwrap();
    } else {
        // A well-formed archive that simply lacks the skill the installer wants.
        std::fs::create_dir_all(top.join("src")).unwrap();
    }

    std::fs::create_dir_all(release_dir).unwrap();
    let tar = Command::new("tar")
        .arg("-czf")
        .arg(release_dir.join(format!("{git_ref}.tar.gz")))
        .arg("-C")
        .arg(&stage)
        .arg(format!("automedon-{git_ref}"))
        .status()
        .unwrap();
    assert!(tar.success(), "failed to build fake source archive");
    std::fs::remove_dir_all(&stage).unwrap();
}

/// Run `install-skill.sh` against a `file://` source. Returns (exit_code, stdout,
/// stderr). `home` isolates `$HOME`; `skills_dir` is the install target.
fn run_install(
    release_dir: &Path,
    home: &Path,
    skills_dir: &Path,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> (i32, String, String) {
    let mut cmd = Command::new("sh");
    cmd.arg(install_skill())
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", home)
        .env(
            "AUTOMEDON_SKILL_BASE_URL",
            format!("file://{}", release_dir.display()),
        )
        .env("AUTOMEDON_SKILLS_DIR", skills_dir);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().expect("failed to run install-skill.sh");
    (
        out.status.code().expect("install-skill.sh killed by signal"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn has_no_syntax_errors() {
    let status = Command::new("sh")
        .arg("-n")
        .arg(install_skill())
        .status()
        .expect("failed to run sh -n");
    assert!(status.success(), "install-skill.sh has syntax errors");
}

#[test]
fn header_documents_conventions_and_overrides() {
    let body = std::fs::read_to_string(install_skill()).expect("install-skill.sh exists");
    assert!(body.starts_with("#!/bin/sh"), "must be a /bin/sh script");
    assert!(body.contains("set -eu"), "must use set -eu");
    assert!(
        body.contains("REPO=\"pop-dog/automedon\""),
        "must target the pop-dog/automedon repo"
    );
    assert!(
        body.contains(".claude/skills"),
        "must install under a .claude/skills path by default"
    );
    assert!(
        body.contains("skills/automedon"),
        "must copy the skills/automedon directory"
    );
    assert!(
        body.contains("AUTOMEDON_SKILLS_DIR"),
        "must expose the documented AUTOMEDON_SKILLS_DIR override"
    );
}

#[test]
fn installs_the_skill_from_a_source_archive() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let skills = TempDir::new("skills");
    fake_archive(&release.0, "main", true);

    let (code, stdout, stderr) = run_install(&release.0, &home.0, &skills.0, &[], &[]);
    assert_eq!(code, 0, "install failed: {stderr}");

    let installed = skills.0.join("automedon").join("SKILL.md");
    assert!(installed.exists(), "skill not installed: {stdout}");
    assert!(
        stdout.contains("automedon") && stdout.contains(skills.0.to_str().unwrap()),
        "missing confirmation line naming the skill and destination: {stdout}"
    );
}

#[test]
fn is_idempotent_replacing_a_prior_install() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let skills = TempDir::new("skills");
    fake_archive(&release.0, "main", true);

    // Pre-seed a stale install with a file that the fresh archive does not carry;
    // a clean replace must remove it rather than leave it behind.
    let dest = skills.0.join("automedon");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("STALE.md"), "old\n").unwrap();

    let (code, _stdout, stderr) = run_install(&release.0, &home.0, &skills.0, &[], &[]);
    assert_eq!(code, 0, "re-install failed: {stderr}");
    assert!(dest.join("SKILL.md").exists(), "fresh skill not installed");
    assert!(
        !dest.join("STALE.md").exists(),
        "stale file from the prior install survived"
    );
}

#[test]
fn honors_a_custom_ref() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let skills = TempDir::new("skills");
    fake_archive(&release.0, "v1.2.3", true);

    let (code, stdout, stderr) =
        run_install(&release.0, &home.0, &skills.0, &[], &["--ref", "v1.2.3"]);
    assert_eq!(code, 0, "install with --ref failed: {stderr}");
    assert!(
        skills.0.join("automedon").join("SKILL.md").exists(),
        "skill not installed from the requested ref: {stdout}"
    );
}

#[test]
fn aborts_when_the_archive_lacks_the_skill() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let skills = TempDir::new("skills");
    fake_archive(&release.0, "main", false);

    let (code, _stdout, stderr) = run_install(&release.0, &home.0, &skills.0, &[], &[]);
    assert_ne!(code, 0, "an archive without the skill should abort");
    assert!(
        stderr.to_lowercase().contains("skills/automedon"),
        "expected an error naming the missing skills/automedon directory: {stderr}"
    );
    assert!(
        !skills.0.join("automedon").exists(),
        "nothing must be installed when the skill is missing"
    );
}
