# Example repo-location helper: `task_repo_cd` lets a Step operate on the
# repository that actually contains its task file, rather than wherever the
# orchestrator happened to be started. This is forkable example code, not a
# Module or crate — copy it next to your Step scripts and `source` it.
#
# The autocoder wrapper's `checkout.sh` runs each task in its own git
# worktree, a sibling directory of the repo the orchestrator was invoked in.
# A coder Step given that worktree's TASK.md path must `cd` there before
# running `git`/`cargo`, or it would act on the wrong working tree. When the
# task file is already inside the invoking repo (the standalone `coder.yaml`
# case), resolving via the task path still lands on that same repo, so the
# `cd` is a no-op.

# task_repo_cd <task-path> — cd into the git repository containing
# <task-path>. Fails loud (returns non-zero, message on stderr) rather than
# falling back to the current directory, so a Step never silently keeps
# operating on the wrong repo.
task_repo_cd() {
    task_repo_cd_root="$(git -C "$(dirname -- "$1")" rev-parse --show-toplevel 2>&1)"
    if [ $? -ne 0 ]; then
        printf 'task_repo_cd: %s is not inside a git repository: %s\n' \
            "$1" "$task_repo_cd_root" >&2
        return 1
    fi
    cd "$task_repo_cd_root" || return 1
}
