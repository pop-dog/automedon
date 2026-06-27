//! Guards the contributor source-install script. The cargo build it performs is
//! too heavy (and network-bound) to run here, so the test confirms the script is
//! syntactically valid and still describes the source build: a `cargo install`
//! of the orchestrator crate plus live symlinks of the bundled skills.

use std::path::PathBuf;
use std::process::Command;

fn dev_install() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/dev-install.sh"
    ))
}

#[test]
fn dev_install_script_has_no_syntax_errors() {
    let status = Command::new("sh")
        .arg("-n")
        .arg(dev_install())
        .status()
        .expect("failed to run sh -n");
    assert!(status.success(), "scripts/dev-install.sh has syntax errors");
}

#[test]
fn dev_install_script_reproduces_the_source_build() {
    let body = std::fs::read_to_string(dev_install()).expect("dev-install.sh exists");
    assert!(body.starts_with("#!/bin/sh"), "must be a /bin/sh script");
    assert!(body.contains("set -eu"), "must use set -eu");
    assert!(
        body.contains("cargo install") && body.contains("crates/orchestrator"),
        "must install the orchestrator crate from source"
    );
    for skill in ["automedon", "autocoder"] {
        assert!(
            body.contains(skill),
            "must symlink the bundled {skill} skill"
        );
    }
}
