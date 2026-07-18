//! Exercises the shipped `examples/lib/repo.sh` example helper directly,
//! mirroring the style of `llm_helper.rs`: source the file in a `/bin/sh`
//! shell and assert on `task_repo_cd`'s effect (the resolved cwd) and its
//! failure mode.
//!
//! `task_repo_cd` lets a coder Step operate on the repository containing its
//! task file even when that repository is a sibling git worktree the
//! orchestrator was not started in (the autocoder wrapper's `checkout.sh`
//! case), while resolving identically to a no-op when the task file is
//! already inside the cwd repo (the standalone `coder.yaml` case).

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn lib_repo() -> PathBuf {
    repo_root().join("examples/lib/repo.sh")
}

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-repo-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Source `examples/lib/repo.sh` and run `body` (a shell fragment) under it
/// with the given starting cwd. Returns (exit_code, stdout, stderr).
fn run_repo(cwd: &std::path::Path, body: &str) -> (i32, String, String) {
    let script = format!(". \"{}\"\n{}", lib_repo().display(), body);
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn sh");
    (
        output.status.code().expect("sh killed by signal"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("failed to run git");
    assert!(status.success(), "git {args:?} failed");
}

/// A main repo with one commit, so `git worktree add` has a HEAD to branch from.
fn init_repo(dir: &std::path::Path) {
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join("README.md"), "x").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

#[test]
fn resolves_a_sibling_worktree_containing_the_task_file() {
    let main = TempDir::new("main");
    init_repo(&main.0);
    let worktree = main.0.parent().unwrap().join(format!("wt-{}", uuid::Uuid::now_v7()));
    git(
        &main.0,
        &[
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "-b",
            "feature",
        ],
    );
    let task = worktree.join("TASK.md");
    std::fs::write(&task, "task").unwrap();

    let (code, stdout, stderr) = run_repo(
        &main.0,
        &format!("task_repo_cd \"{}\" && pwd", task.display()),
    );
    assert_eq!(code, 0, "task_repo_cd should succeed; stderr:\n{stderr}");
    let resolved = std::fs::canonicalize(stdout.trim()).unwrap();
    let expected = std::fs::canonicalize(&worktree).unwrap();
    assert_eq!(resolved, expected, "should cd into the worktree root");

    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree.to_str().unwrap()])
        .current_dir(&main.0)
        .status();
}

#[test]
fn resolves_identically_when_the_task_file_is_already_in_the_cwd_repo() {
    let main = TempDir::new("standalone");
    init_repo(&main.0);
    let task = main.0.join("TASK.md");
    std::fs::write(&task, "task").unwrap();

    let (code, stdout, stderr) = run_repo(
        &main.0,
        &format!("task_repo_cd \"{}\" && pwd", task.display()),
    );
    assert_eq!(code, 0, "task_repo_cd should succeed; stderr:\n{stderr}");
    let resolved = std::fs::canonicalize(stdout.trim()).unwrap();
    let expected = std::fs::canonicalize(&main.0).unwrap();
    assert_eq!(resolved, expected, "should be a no-op cd for the cwd repo");
}

#[test]
fn errors_loudly_when_the_task_file_is_not_in_a_git_repository() {
    let plain = TempDir::new("non-repo");
    let task = plain.0.join("TASK.md");
    std::fs::write(&task, "task").unwrap();

    let (code, stdout, stderr) = run_repo(
        &plain.0,
        &format!("task_repo_cd \"{}\"", task.display()),
    );
    assert_ne!(code, 0, "task_repo_cd must fail outside a git repository");
    assert_eq!(stdout, "", "no partial output on failure");
    assert!(
        stderr.contains(&task.display().to_string()) || stderr.contains("git repository"),
        "stderr should explain the failure; got:\n{stderr}"
    );
}
