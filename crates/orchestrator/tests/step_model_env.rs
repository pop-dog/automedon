//! Exercises `examples/coder/commit.sh` directly (no `CODER_STUB`) against a
//! fake `claude` on `PATH` that records its argv, proving the declared
//! per-Step `model` (issue #60) is what actually reaches `claude --model`
//! rather than the retired `CODER_COMMIT_MODEL` knob.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn commit_script() -> PathBuf {
    repo_root().join("examples/coder/commit.sh")
}

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-model-env-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fake `claude` executable on its own `PATH` directory that appends each
/// argv entry (one per line) to a log file, so the test can inspect exactly
/// what `--model` value the script passed without invoking the real CLI.
struct FakeClaude {
    _dir: TempDir,
    bin_dir: PathBuf,
    log: PathBuf,
}

impl FakeClaude {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let bin_dir = dir.0.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let log = dir.0.join("claude.log");
        let claude_path = bin_dir.join("claude");
        std::fs::write(
            &claude_path,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> \"{}\"; done\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(
            &claude_path,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        FakeClaude { _dir: dir, bin_dir, log }
    }

    /// `PATH` with the fake `claude` ahead of the real one, so the script's
    /// unqualified `claude` invocation resolves to the fake.
    fn path_env(&self) -> String {
        let real_path = std::env::var("PATH").unwrap_or_default();
        format!("{}:{real_path}", self.bin_dir.display())
    }

    /// The `--model` value the script passed to `claude`, if any.
    fn model_arg(&self) -> Option<String> {
        let contents = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = contents.lines().collect();
        lines
            .iter()
            .position(|&l| l == "--model")
            .and_then(|i| lines.get(i + 1))
            .map(|s| s.to_string())
    }
}

/// Run `commit.sh` for a throwaway task path inside this repo (so
/// `task_repo_cd` resolves) with `extra_env` layered over the required
/// engine-provided variables, returning the fake `claude`'s recorded
/// `--model` argument.
fn commit_model_arg(extra_env: &[(&str, &str)]) -> Option<String> {
    let run_dir = TempDir::new("run");
    let fake_claude = FakeClaude::new("commit");
    let task_path = repo_root().join("TASK.md");

    let mut cmd = Command::new("/bin/sh");
    cmd.arg(commit_script())
        .current_dir(repo_root())
        .env("PATH", fake_claude.path_env())
        .env("AUTOMEDON_WORKFLOW_DIR", repo_root().join("examples"))
        .env("AUTOMEDON_RUN_DIR", &run_dir.0)
        .env_remove("CODER_STUB");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn commit.sh");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(task_path.to_str().unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("failed to wait on commit.sh");
    assert!(
        output.status.success(),
        "commit.sh should succeed against the fake claude; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    fake_claude.model_arg()
}

#[test]
fn declared_model_reaches_claude_model_flag() {
    // The Step's declared `model` (the Workflow's `env:`, injected exactly
    // like this test's `env`) must be the value `claude --model` receives —
    // not the retired `CODER_COMMIT_MODEL` knob.
    let model_arg = commit_model_arg(&[("model", "opus")]);
    assert_eq!(
        model_arg.as_deref(),
        Some("opus"),
        "commit.sh should pass the declared `model` env var to claude --model"
    );
}

#[test]
fn stale_coder_commit_model_no_longer_has_any_effect() {
    // The old, script-namespaced knob is retired; setting it must not
    // change what reaches claude — only `model` may.
    let model_arg = commit_model_arg(&[("CODER_COMMIT_MODEL", "sonnet")]);
    assert_ne!(
        model_arg.as_deref(),
        Some("sonnet"),
        "CODER_COMMIT_MODEL should no longer influence commit.sh's claude invocation"
    );
}

#[test]
fn commit_defaults_to_haiku_when_no_model_is_declared() {
    let model_arg = commit_model_arg(&[]);
    assert_eq!(model_arg.as_deref(), Some("haiku"));
}
