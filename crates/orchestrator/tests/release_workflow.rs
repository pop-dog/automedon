//! Guards the release workflow that produces the binaries `install.sh`
//! downloads. The asset names the workflow attaches must line up with the
//! `automedon-<version>-<os>-<arch>.tar.gz` names the installer requests, so
//! this test pins both ends of that contract.

use std::path::PathBuf;

fn read_workflow() -> String {
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../.github/workflows/release.yml"
    ));
    std::fs::read_to_string(path).expect(".github/workflows/release.yml exists")
}

#[test]
fn release_workflow_is_valid_yaml() {
    let body = read_workflow();
    serde_yaml::from_str::<serde_yaml::Value>(&body).expect("release.yml must be valid YAML");
}

#[test]
fn release_workflow_triggers_on_version_tags() {
    let body = read_workflow();
    assert!(
        body.contains("tags") && body.contains("v*"),
        "must trigger on pushed v* tags"
    );
}

#[test]
fn release_workflow_builds_the_supported_targets() {
    let body = read_workflow();
    // macOS ships Apple Silicon only; there is no Intel (x86_64) Darwin build.
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ] {
        assert!(body.contains(triple), "missing build target {triple}");
    }
    assert!(
        !body.contains("x86_64-apple-darwin"),
        "Intel macOS is intentionally unsupported"
    );
}

#[test]
fn release_workflow_attaches_the_installer_assets() {
    let body = read_workflow();
    // The os-arch labels the installer derives from `uname`.
    for label in ["linux-x86_64", "linux-aarch64", "darwin-aarch64"] {
        assert!(
            body.contains(label),
            "missing release asset for {label}"
        );
    }
    assert!(
        body.contains("automedon-") && body.contains(".tar.gz"),
        "assets must follow the automedon-<version>-<os>-<arch>.tar.gz pattern"
    );
    assert!(
        body.contains("checksums.sha256"),
        "must attach checksums.sha256 alongside the archives"
    );
}
