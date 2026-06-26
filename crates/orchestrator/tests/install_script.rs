//! Exercises the remote `install.sh` end-to-end without touching the network.
//!
//! The script's release host (`AUTOMEDON_BASE_URL`) and latest-release endpoint
//! (`AUTOMEDON_API_URL`) are override points, so the tests stand up a fake
//! release as plain files and point the script at it with a `file://` URL. The
//! installed binary is a throwaway shell stub, so "did it install and is it
//! runnable" is observable without a real build.

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

/// Map this host the same way `install.sh` does, so the fake release advertises
/// the archive name the script will actually request.
fn host_target() -> (String, String) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        other => panic!("unsupported test host OS: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => panic!("unsupported test host arch: {other}"),
    };
    (os.to_string(), arch.to_string())
}

/// Checksum `archive` from within `dir`, preferring `sha256sum` and falling back
/// to `shasum -a 256` (as `install.sh` does). Returns the `<hash>  <name>` line.
fn sha256sum(dir: &Path, archive: &str) -> Vec<u8> {
    let attempt = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .arg(archive)
            .current_dir(dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout)
    };
    attempt("sha256sum", &[])
        .or_else(|| attempt("shasum", &["-a", "256"]))
        .expect("need sha256sum or shasum to checksum the fake archive")
}

/// Lay down a fake release directory (`<dir>/<tag>/...`) holding the archive the
/// installer will fetch plus a matching `checksums.sha256`. `corrupt` rewrites
/// the archive after the checksum is recorded, simulating a tampered download.
fn fake_release(release_dir: &Path, version: &str, corrupt: bool) {
    let (os, arch) = host_target();
    let tag = format!("v{version}");
    let archive = format!("automedon-{version}-{os}-{arch}.tar.gz");
    let tag_dir = release_dir.join(&tag);
    std::fs::create_dir_all(&tag_dir).unwrap();

    // A runnable stub stands in for the real binary; running it proves the
    // installer extracted, placed, and chmod'd it.
    let stage = release_dir.join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(
        stage.join("automedon"),
        format!("#!/bin/sh\necho \"automedon {version}\"\n"),
    )
    .unwrap();

    let tar = Command::new("tar")
        .arg("-czf")
        .arg(tag_dir.join(&archive))
        .arg("-C")
        .arg(&stage)
        .arg("automedon")
        .status()
        .unwrap();
    assert!(tar.success(), "failed to build fake archive");

    // Mirror the script's own `sha256sum`-or-`shasum` selection so the fixture
    // works on macOS hosts, which ship `shasum` but not `sha256sum`.
    let sum = sha256sum(&tag_dir, &archive);
    std::fs::write(tag_dir.join("checksums.sha256"), &sum).unwrap();

    if corrupt {
        // Overwrite the archive so its bytes no longer match the recorded hash.
        std::fs::write(tag_dir.join(&archive), b"tampered\n").unwrap();
    }
}

/// Run `install.sh` against a `file://` release. Returns (exit_code, stdout,
/// stderr). `home` isolates `$HOME`; `extra_env` scripts overrides and flags are
/// passed as trailing args.
fn run_install(
    release_dir: &Path,
    home: &Path,
    bin_dir: &Path,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> (i32, String, String) {
    let mut cmd = Command::new("sh");
    cmd.arg(repo_root().join("install.sh"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("HOME", home)
        .env(
            "AUTOMEDON_BASE_URL",
            format!("file://{}", release_dir.display()),
        )
        .env("AUTOMEDON_BIN_DIR", bin_dir);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().expect("failed to run install.sh");
    (
        out.status.code().expect("install.sh killed by signal"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn downloads_verifies_and_installs_the_binary() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let bin = TempDir::new("bin");
    fake_release(&release.0, "0.1.0", false);

    let (code, stdout, stderr) =
        run_install(&release.0, &home.0, &bin.0, &[("VERSION", "0.1.0")], &[]);
    assert_eq!(code, 0, "install failed: {stderr}");

    let installed = bin.0.join("automedon");
    assert!(installed.exists(), "binary not installed: {stdout}");

    let run = Command::new(&installed).output().unwrap();
    assert!(run.status.success(), "installed binary not runnable");
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "automedon 0.1.0");

    assert!(
        stdout.contains("0.1.0") && stdout.contains(installed.to_str().unwrap()),
        "missing confirmation line: {stdout}"
    );
}

#[test]
fn aborts_on_checksum_mismatch() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let bin = TempDir::new("bin");
    fake_release(&release.0, "0.1.0", true);

    let (code, _stdout, stderr) =
        run_install(&release.0, &home.0, &bin.0, &[("VERSION", "0.1.0")], &[]);
    assert_ne!(code, 0, "tampered download should abort");
    assert!(
        stderr.to_lowercase().contains("checksum"),
        "expected a checksum error: {stderr}"
    );
    assert!(
        !bin.0.join("automedon").exists(),
        "binary must not be installed after a failed checksum"
    );
}

#[test]
fn rejects_unsupported_architecture() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let bin = TempDir::new("bin");
    let fake = TempDir::new("fakebin");

    // A `uname` that reports an architecture the installer cannot map. Shadowing
    // it on PATH lets the test drive platform detection without a real exotic
    // host.
    std::fs::write(
        fake.0.join("uname"),
        "#!/bin/sh\ncase \"$1\" in\n -s) echo Linux ;;\n -m) echo riscv64 ;;\nesac\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(fake.0.join("uname")).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    std::fs::set_permissions(fake.0.join("uname"), perms).unwrap();

    let shadowed_path = format!("{}:{}", fake.0.display(), std::env::var("PATH").unwrap());
    let (code, _stdout, stderr) = run_install(
        &release.0,
        &home.0,
        &bin.0,
        &[("VERSION", "0.1.0"), ("PATH", &shadowed_path)],
        &[],
    );
    assert_ne!(code, 0, "unsupported arch should abort");
    assert!(
        stderr.contains("riscv64"),
        "expected an architecture error naming riscv64: {stderr}"
    );
}

#[test]
fn warns_when_install_dir_is_off_path() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let bin = TempDir::new("bin");
    fake_release(&release.0, "0.1.0", false);

    let (code, stdout, _stderr) =
        run_install(&release.0, &home.0, &bin.0, &[("VERSION", "0.1.0")], &[]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("export PATH=") && stdout.contains(bin.0.to_str().unwrap()),
        "off-PATH install should print guidance: {stdout}"
    );
}

#[test]
fn resolves_latest_version_from_the_release_api() {
    let home = TempDir::new("home");
    let release = TempDir::new("release");
    let bin = TempDir::new("bin");
    fake_release(&release.0, "0.1.0", false);

    // Stand in for the GitHub "latest release" endpoint.
    let api = release.0.join("latest.json");
    std::fs::write(&api, "{\n  \"tag_name\": \"v0.1.0\"\n}\n").unwrap();
    let api_url = format!("file://{}", api.display());

    // No VERSION: the script must read the tag from the API instead.
    let (code, stdout, stderr) =
        run_install(&release.0, &home.0, &bin.0, &[("AUTOMEDON_API_URL", &api_url)], &[]);
    assert_eq!(code, 0, "latest resolution failed: {stderr}");
    assert!(bin.0.join("automedon").exists(), "binary not installed: {stdout}");
    assert!(stdout.contains("0.1.0"), "missing version in confirmation: {stdout}");
}
