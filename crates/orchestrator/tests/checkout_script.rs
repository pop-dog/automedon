//! Exercises `examples/autocoder/checkout.sh` directly against a throwaway
//! git repository in a temp dir, mirroring the style of `llm_helper.rs` /
//! `repo_helper.rs`: run the real script (no stub), assert on its stdout
//! (the out-Message) and exit code, and inspect the resulting worktree
//! layout on disk. This is the only git-mutating Step in the autocoder
//! wrapper, so it is the one exercised against real git rather than stubbed.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn checkout_script() -> PathBuf {
    repo_root().join("examples/autocoder/checkout.sh")
}

/// A throwaway directory under the system temp dir, removed on Drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ao-checkout-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A throwaway git repository nested one level inside its own unique
/// container directory, so `../automedon.worktrees` (a sibling of the
/// repo's toplevel) does not collide across tests sharing the system temp
/// root.
struct TempRepo {
    _container: TempDir,
    path: PathBuf,
}

impl TempRepo {
    fn new(tag: &str) -> Self {
        let container = TempDir::new(tag);
        let path = container.0.join("repo");
        std::fs::create_dir_all(&path).unwrap();
        init_repo(&path);
        TempRepo { _container: container, path }
    }
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

fn init_repo(dir: &std::path::Path) {
    git(dir, &["init", "-q"]);
    std::fs::write(dir.join("README.md"), "x").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

/// Run `checkout.sh` with `branch` as the in-Message from `repo`, staging
/// `staged_task_md` (if given) at `$AUTOMEDON_RUN_DIR/TASK.md` first.
/// Returns (exit_code, stdout, stderr).
fn run_checkout(
    repo: &std::path::Path,
    run_dir: &std::path::Path,
    branch: &str,
    staged_task_md: Option<&str>,
) -> (i32, String, String) {
    if let Some(body) = staged_task_md {
        std::fs::write(run_dir.join("TASK.md"), body).unwrap();
    }
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(checkout_script())
        .current_dir(repo)
        .env("AUTOMEDON_RUN_DIR", run_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn checkout.sh");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(branch.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("failed to wait on checkout.sh");
    (
        out.status.code().expect("checkout.sh killed by signal"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn worktrees_root(repo: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(repo)
        .unwrap()
        .parent()
        .unwrap()
        .join("automedon.worktrees")
}

#[test]
fn fresh_run_creates_a_worktree_and_moves_the_staged_task_md() {
    let repo = TempRepo::new("fresh-repo");
    let run_dir = TempDir::new("fresh-run");

    let (code, stdout, stderr) = run_checkout(
        &repo.path,
        &run_dir.0,
        "fix/14-prune-race",
        Some("# Prune race\n"),
    );
    assert_eq!(code, 0, "checkout should succeed; stderr:\n{stderr}");

    let worktree = worktrees_root(&repo.path).join("14-prune-race");
    let expected_task_md = worktree.join(".workflows/14-prune-race/TASK.md");
    assert_eq!(
        stdout,
        expected_task_md.display().to_string(),
        "should emit the worktree-absolute TASK.md path"
    );
    assert!(worktree.is_dir(), "worktree directory should exist");
    assert!(expected_task_md.is_file(), "TASK.md should be moved into the worktree");
    assert_eq!(
        std::fs::read_to_string(&expected_task_md).unwrap(),
        "# Prune race\n"
    );
    assert!(
        !run_dir.0.join("TASK.md").exists(),
        "the staged TASK.md should be moved, not copied"
    );

    let branch_out = Command::new("git")
        .args(["branch", "--list", "fix/14-prune-race"])
        .current_dir(&repo.path)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_out.stdout).contains("fix/14-prune-race"),
        "the branch should have been created"
    );

    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree.to_str().unwrap()])
        .current_dir(&repo.path)
        .status();
}

#[test]
fn recovery_reuses_the_existing_worktree_and_branch() {
    let repo = TempRepo::new("recover-repo");
    let run_dir = TempDir::new("recover-run");

    let (code, stdout, stderr) = run_checkout(
        &repo.path,
        &run_dir.0,
        "fix/9-flaky-test",
        Some("# Flaky test\n"),
    );
    assert_eq!(code, 0, "first run should succeed; stderr:\n{stderr}");
    let worktree = worktrees_root(&repo.path).join("9-flaky-test");

    // Re-emit the same branch name, as distill's recovery path does, with no
    // staged TASK.md this time (nothing new was fetched).
    let (code2, stdout2, stderr2) = run_checkout(&repo.path, &run_dir.0, "fix/9-flaky-test", None);
    assert_eq!(code2, 0, "recovery run should succeed; stderr:\n{stderr2}");
    assert_eq!(stdout, stdout2, "recovery should re-emit the same TASK.md path");
    assert!(
        worktree.join(".workflows/9-flaky-test/TASK.md").is_file(),
        "the original TASK.md should still be in place"
    );

    let _ = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", worktree.to_str().unwrap()])
        .current_dir(&repo.path)
        .status();
}

#[test]
fn collision_branch_exists_without_this_issues_task_md_fails_closed() {
    let repo = TempRepo::new("branch-only-repo");
    // A branch that already exists for this "issue" but was never checked
    // out into a worktree by this Step (no TASK.md ever landed for it).
    git(&repo.path, &["branch", "fix/5-orphan-branch"]);
    let run_dir = TempDir::new("branch-only-run");

    let (code, stdout, stderr) = run_checkout(
        &repo.path,
        &run_dir.0,
        "fix/5-orphan-branch",
        Some("# Orphan\n"),
    );
    assert_ne!(code, 0, "an existing branch without this issue's TASK.md must fail closed");
    assert_eq!(stdout, "", "no path should be emitted on failure");
    assert!(!stderr.is_empty(), "stderr should explain the collision");
}

#[test]
fn collision_unregistered_worktree_directory_fails_closed() {
    let repo = TempRepo::new("unregistered-repo");
    let worktree = worktrees_root(&repo.path).join("3-manual-dir");
    std::fs::create_dir_all(&worktree).unwrap();
    let run_dir = TempDir::new("unregistered-run");

    let (code, stdout, stderr) =
        run_checkout(&repo.path, &run_dir.0, "fix/3-manual-dir", Some("# Manual\n"));
    assert_ne!(code, 0, "an unregistered directory at the expected path must fail closed");
    assert_eq!(stdout, "", "no path should be emitted on failure");
    assert!(!stderr.is_empty(), "stderr should explain the collision");
}

#[test]
fn collision_branch_checked_out_elsewhere_fails_closed() {
    let repo = TempRepo::new("elsewhere-repo");
    // Check the branch out directly in the main worktree (not via this
    // Step), so it is "checked out elsewhere" relative to the expected
    // sibling worktree path.
    git(&repo.path, &["checkout", "-q", "-b", "fix/7-elsewhere"]);
    let run_dir = TempDir::new("elsewhere-run");

    let (code, stdout, stderr) =
        run_checkout(&repo.path, &run_dir.0, "fix/7-elsewhere", Some("# Elsewhere\n"));
    assert_ne!(code, 0, "a branch checked out elsewhere must fail closed");
    assert_eq!(stdout, "", "no path should be emitted on failure");
    assert!(!stderr.is_empty(), "stderr should explain the collision");
}

#[test]
fn prune_clears_a_stale_registration_left_by_a_hand_removed_worktree() {
    let repo = TempRepo::new("prune-repo");
    let run_dir = TempDir::new("prune-run");

    // A worktree from an unrelated, already-cleaned-up issue: its directory
    // was removed by hand (not via `git worktree remove`), leaving git's own
    // registration dangling — exactly what `git worktree prune` cleans up.
    let stale_worktree = worktrees_root(&repo.path).join("2-stale");
    std::fs::create_dir_all(stale_worktree.parent().unwrap()).unwrap();
    git(
        &repo.path,
        &[
            "worktree",
            "add",
            stale_worktree.to_str().unwrap(),
            "-b",
            "fix/2-stale",
        ],
    );
    std::fs::remove_dir_all(&stale_worktree).unwrap();
    let before = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo.path)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&before.stdout).contains("2-stale"),
        "sanity: the stale registration should still be present before this run"
    );

    let (code, _stdout, stderr) =
        run_checkout(&repo.path, &run_dir.0, "fix/14-new-issue", Some("# New issue\n"));
    assert_eq!(code, 0, "checkout should succeed; stderr:\n{stderr}");

    let after = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo.path)
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&after.stdout).contains("2-stale"),
        "checkout.sh should have pruned the stale registration"
    );

    let worktree = worktrees_root(&repo.path).join("14-new-issue");
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", worktree.to_str().unwrap()])
        .current_dir(&repo.path)
        .status();
}
