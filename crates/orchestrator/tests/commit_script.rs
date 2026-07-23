//! Exercises `examples/coder/commit.sh` directly against a throwaway git repo
//! in a temp dir, mirroring `checkout_script.rs`: run the real script (no stub)
//! with a fake `claude` on `PATH`, and assert on its stdout (the out-Message),
//! exit code, and the resulting commit — chiefly that staging includes files
//! `code` created, not only tracked edits.

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
        let path = std::env::temp_dir().join(format!("ao-commit-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        // Isolate from host gitconfig so a global commit.gpgsign or hook
        // can't fail the test's own setup.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
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

/// A `claude` stand-in on `PATH` that records its argv to `dir/claude.argv`
/// (so a test can read what `--model` it was given) and prints `message` to
/// stdout, mirroring how the real CLI's `-p` reply lands on stdout. Placed in
/// its own directory so it can be prepended to `PATH` without shadowing
/// anything else `commit.sh` needs (git, jq, sh).
fn fake_claude_bin(dir: &std::path::Path, message: &str) -> PathBuf {
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let log = dir.join("claude.argv").display().to_string();
    let script = bin_dir.join("claude");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> {log:?}; done\nprintf '%s' {message:?}\n"),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    bin_dir
}

/// The `--model` value the fake `claude` in `dir` recorded, if any.
fn recorded_model(dir: &std::path::Path) -> Option<String> {
    let log = std::fs::read_to_string(dir.join("claude.argv")).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    lines
        .iter()
        .position(|&l| l == "--model")
        .and_then(|i| lines.get(i + 1))
        .map(|s| s.to_string())
}

const COMMIT_MESSAGE: &str = "fix: cover untracked files in commit staging";

/// Run `commit.sh` with `task_path` as the in-Message, from inside `repo`,
/// with the fake `claude` (if any) ahead of the real `PATH`. Returns
/// (exit_code, stdout, stderr).
fn run_commit(
    repo: &std::path::Path,
    run_dir: &std::path::Path,
    task_path: &std::path::Path,
    fake_claude_dir: Option<&std::path::Path>,
) -> (i32, String, String) {
    run_commit_from(repo, run_dir, &task_path.display().to_string(), fake_claude_dir, &[])
}

/// Like `run_commit`, but lets the caller give the in-Message task path as a
/// literal string (e.g. a relative path) and layer `extra_env` onto the Step's
/// environment (e.g. the per-Step `model` knob).
fn run_commit_from(
    cwd: &std::path::Path,
    run_dir: &std::path::Path,
    task_path: &str,
    fake_claude_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
) -> (i32, String, String) {
    let path = match fake_claude_dir {
        Some(dir) => format!("{}:{}", dir.display(), std::env::var("PATH").unwrap()),
        None => std::env::var("PATH").unwrap(),
    };
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(commit_script())
        .current_dir(cwd)
        .env("AUTOMEDON_RUN_DIR", run_dir)
        .env("AUTOMEDON_WORKFLOW_DIR", repo_root().join("examples"))
        .env("PATH", path)
        // commit.sh runs `git commit` itself, so give it an identity that does
        // not depend on host gitconfig (CI and fresh machines have none).
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let mut child = cmd.spawn().expect("failed to spawn commit.sh");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(task_path.as_bytes())
        .unwrap();
    let out = child
        .wait_with_output()
        .expect("failed to wait on commit.sh");
    (
        out.status.code().expect("commit.sh killed by signal"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn head_files(repo: &std::path::Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["show", "--pretty=", "--name-only", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

fn head_message(repo: &std::path::Path) -> String {
    let out = Command::new("git")
        .args(["log", "-1", "--pretty=%B"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn head_hash(repo: &std::path::Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn untracked_and_modified_files_are_both_committed() {
    let container = TempDir::new("untracked-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    // A tracked-file edit plus a brand-new untracked file — the case
    // `git diff HEAD` would never surface.
    std::fs::write(repo.join("README.md"), "modified").unwrap();
    std::fs::write(repo.join("new_file.rs"), "fn main() {}").unwrap();

    let run_dir = TempDir::new("untracked-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();

    let (code, stdout, stderr) = run_commit(&repo, &run_dir.0, &task_path, Some(&claude_dir));
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");

    let files = head_files(&repo);
    assert!(
        files.iter().any(|f| f == "new_file.rs"),
        "the untracked file should be committed; HEAD files: {files:?}"
    );
    assert!(
        files.iter().any(|f| f == "README.md"),
        "the modified tracked file should be committed; HEAD files: {files:?}"
    );

    assert_eq!(
        stdout,
        task_path.display().to_string(),
        "stdout should carry only the task path"
    );
}

#[test]
fn commit_message_never_leaks_onto_stdout() {
    let container = TempDir::new("stdout-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    std::fs::write(repo.join("README.md"), "modified").unwrap();

    let run_dir = TempDir::new("stdout-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();

    let (code, stdout, stderr) = run_commit(&repo, &run_dir.0, &task_path, Some(&claude_dir));
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");
    assert!(
        !stdout.contains(COMMIT_MESSAGE),
        "the commit message must never appear on stdout; got:\n{stdout}"
    );
    assert_eq!(head_message(&repo).trim(), COMMIT_MESSAGE);
}

#[test]
fn standalone_task_file_is_never_committed() {
    // Standalone `coder.yaml` mode points --message at a real TASK.md inside
    // the repo, not the autocoder's gitignored `.workflows/`; `git add -A`
    // must not sweep it into the deliverable.
    let container = TempDir::new("standalone-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    std::fs::write(repo.join("README.md"), "modified").unwrap();
    std::fs::write(repo.join("new_file.rs"), "fn main() {}").unwrap();
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();

    let run_dir = TempDir::new("standalone-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);

    let (code, _stdout, stderr) = run_commit(&repo, &run_dir.0, &task_path, Some(&claude_dir));
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");

    let files = head_files(&repo);
    assert!(
        !files.iter().any(|f| f == "TASK.md"),
        "the in-Message task file should never be committed as a deliverable; HEAD files: {files:?}"
    );
    assert!(
        files.iter().any(|f| f == "new_file.rs"),
        "the untracked deliverable should still be committed; HEAD files: {files:?}"
    );
    assert!(
        files.iter().any(|f| f == "README.md"),
        "the modified tracked deliverable should still be committed; HEAD files: {files:?}"
    );
}

#[test]
fn relative_task_path_from_parent_does_not_abort() {
    // task_repo_cd moves cwd to the repo toplevel; if the in-Message path is
    // then used unresolved, a path relative to the original cwd (a
    // subdirectory) points outside the repo and `git reset` fails.
    let container = TempDir::new("relative-parent-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(repo.join("new_file.rs"), "fn main() {}").unwrap();
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();
    // Committed so the repo starts clean except for new_file.rs and TASK.md,
    // mirroring the standalone entry mode where TASK.md is tracked.
    git(&repo, &["add", "TASK.md"]);
    git(&repo, &["commit", "-q", "-m", "seed task"]);

    let run_dir = TempDir::new("relative-parent-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);

    let (code, stdout, stderr) =
        run_commit_from(&sub, &run_dir.0, "../TASK.md", Some(&claude_dir), &[]);
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");
    assert_eq!(
        stdout, "../TASK.md",
        "stdout should carry the in-Message path verbatim"
    );

    let files = head_files(&repo);
    assert!(
        files.iter().any(|f| f == "new_file.rs"),
        "the untracked deliverable should be committed; HEAD files: {files:?}"
    );
}

#[test]
fn relative_task_path_in_cwd_is_never_committed() {
    // Same bug, the other failure mode: a relative in-Message path that
    // happens to resolve (from the post-cd toplevel) to nothing must not
    // let `git reset` silently miss the real task file, landing it in the
    // commit.
    let container = TempDir::new("relative-cwd-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    let sub = repo.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(repo.join("new_file.rs"), "fn main() {}").unwrap();
    std::fs::write(sub.join("TASK.md"), "# Task\n").unwrap();

    let run_dir = TempDir::new("relative-cwd-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);

    let (code, _stdout, stderr) =
        run_commit_from(&sub, &run_dir.0, "./TASK.md", Some(&claude_dir), &[]);
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");

    let files = head_files(&repo);
    assert!(
        !files.iter().any(|f| f == "sub/TASK.md"),
        "the in-Message task file should never be committed as a deliverable; HEAD files: {files:?}"
    );
    assert!(
        files.iter().any(|f| f == "new_file.rs"),
        "the untracked deliverable should still be committed; HEAD files: {files:?}"
    );
}

#[test]
fn empty_agent_reply_fails_and_leaves_tree_unstaged() {
    let container = TempDir::new("empty-reply-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    std::fs::write(repo.join("README.md"), "modified").unwrap();

    let run_dir = TempDir::new("empty-reply-run");
    let claude_dir = fake_claude_bin(&container.0, "");
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();
    let before = head_hash(&repo);

    let (code, stdout, stderr) = run_commit(&repo, &run_dir.0, &task_path, Some(&claude_dir));
    assert_ne!(code, 0, "an empty commit message must escalate");
    assert_eq!(stdout, "", "no path should be emitted on failure");
    assert!(
        !stderr.is_empty(),
        "stderr should explain the empty-reply escalation"
    );
    assert_eq!(
        head_hash(&repo),
        before,
        "no commit should have been created"
    );

    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "the tree should be left unstaged after an escalation, not half-committed"
    );
}

#[test]
fn never_pushes() {
    let commit_sh = std::fs::read_to_string(commit_script()).unwrap();
    assert!(
        !commit_sh.contains("git push"),
        "commit.sh should never invoke git push; got:\n{commit_sh}"
    );
}

#[test]
fn clean_tree_escalates_instead_of_committing() {
    let container = TempDir::new("clean-repo");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);

    let run_dir = TempDir::new("clean-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();
    // The staged TASK.md write above would itself be a change to stage, so
    // commit it first, leaving a genuinely clean working tree for the Step
    // to observe.
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "seed task"]);
    let before = head_hash(&repo);

    let (code, stdout, stderr) = run_commit(&repo, &run_dir.0, &task_path, Some(&claude_dir));
    assert_ne!(code, 0, "a clean tree must escalate rather than commit");
    assert_eq!(stdout, "", "no path should be emitted on failure");
    assert!(
        !stderr.is_empty(),
        "stderr should explain the empty-tree escalation"
    );
    assert_eq!(
        head_hash(&repo),
        before,
        "no commit should have been created"
    );
}

#[test]
fn prompt_no_longer_delegates_to_commit_skill() {
    let prompt =
        std::fs::read_to_string(repo_root().join("examples/coder/prompts/commit.md")).unwrap();
    assert!(
        !prompt.contains("/commit"),
        "commit.md should no longer delegate staging to the /commit skill; got:\n{prompt}"
    );
}

#[test]
fn permissions_deny_every_git_write() {
    let path = repo_root().join("examples/coder/commit.permissions.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let deny: Vec<&str> = json["permissions"]["deny"]
        .as_array()
        .expect("deny should be an array")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for rule in [
        "Bash(git add*)",
        "Bash(git commit*)",
        "Bash(git push*)",
        "Edit",
        "Write",
    ] {
        assert!(
            deny.contains(&rule),
            "commit.permissions.json should deny {rule}; got: {deny:?}"
        );
    }
}

/// Run `commit.sh` over a throwaway repo with one staged deliverable and
/// `extra_env` layered on, returning the `--model` the fake `claude` received.
fn commit_model_arg(extra_env: &[(&str, &str)]) -> Option<String> {
    let container = TempDir::new("model");
    let repo = container.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    std::fs::write(repo.join("new_file.rs"), "fn main() {}").unwrap();
    let run_dir = TempDir::new("model-run");
    let claude_dir = fake_claude_bin(&container.0, COMMIT_MESSAGE);
    let task_path = repo.join("TASK.md");
    std::fs::write(&task_path, "# Task\n").unwrap();

    let (code, _stdout, stderr) = run_commit_from(
        &repo,
        &run_dir.0,
        &task_path.display().to_string(),
        Some(&claude_dir),
        extra_env,
    );
    assert_eq!(code, 0, "commit should succeed; stderr:\n{stderr}");
    recorded_model(&container.0)
}

#[test]
fn declared_model_reaches_claude_model_flag() {
    // The per-Step `model` env (issue #58) is what `claude --model` receives.
    assert_eq!(commit_model_arg(&[("model", "opus")]).as_deref(), Some("opus"));
}

#[test]
fn a_bare_model_falls_back_to_the_default() {
    // Only `model` selects the model: with none declared, commit.sh's own
    // `${model:-haiku}` default reaches claude, and an unrelated env var
    // (here the pre-rename CODER_COMMIT_MODEL) cannot override it.
    assert_eq!(
        commit_model_arg(&[("CODER_COMMIT_MODEL", "sonnet")]).as_deref(),
        Some("haiku")
    );
}
