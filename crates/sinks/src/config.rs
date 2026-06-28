//! Resolving where Runs are logged and how many to retain. Kept as pure
//! functions (no env/argv access) so the precedence rules are unit-testable
//! (ADR-0009, decisions 4 and 7).

use std::path::{Path, PathBuf};

use kernel::DEFAULT_MAX_DEPTH;

/// Application subdirectory under the chosen state root.
const APP_DIR: &str = "automedon";

/// Default number of Runs to retain when no override is given.
pub const DEFAULT_KEEP: usize = 100;

/// Resolve the directory that holds per-Run subdirectories.
///
/// Precedence: an explicit `override_dir` (from `--log-dir`/env) is used
/// verbatim; otherwise `$XDG_STATE_HOME/automedon/runs`; otherwise the
/// XDG default `~/.local/state/automedon/runs`. With no home either, a
/// relative path is the last resort so a Run can still be logged.
pub fn runs_dir(
    override_dir: Option<&str>,
    xdg_state_home: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    if let Some(dir) = override_dir {
        return PathBuf::from(dir);
    }
    let state_root = match (xdg_state_home, home) {
        (Some(xdg), _) => PathBuf::from(xdg),
        (None, Some(home)) => PathBuf::from(home).join(".local").join("state"),
        (None, None) => PathBuf::new(),
    };
    state_root.join(APP_DIR).join("runs")
}

/// Resolve a Run's ephemeral Run Directory (`$AUTOMEDON_RUN_DIR`):
/// `<temp_root>/automedon/runs/<run-id>/` (ADR-0010). It shares its
/// `run_id` with the durable log dir but lives under the OS temp root, not state,
/// so the two correlate yet have independent lifecycles. Pure (the caller passes
/// `std::env::temp_dir()`) so the layout is unit-testable without env access.
pub fn run_scratch_dir(temp_root: &Path, run_id: &str) -> PathBuf {
    temp_root.join(APP_DIR).join("runs").join(run_id)
}

/// Resolve the retention cap: a `--keep`/env value if it parses, else
/// [`DEFAULT_KEEP`]. An unparseable override falls back to the default rather
/// than failing the Run. The cap is at least 1: the active Run is the newest
/// entry and pruning keeps the newest `keep`, so 0 would delete the log the Run
/// is currently writing.
pub fn resolve_keep(flag: Option<&str>, env: Option<&str>) -> usize {
    flag.or(env)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_KEEP)
        .max(1)
}

/// Resolve the max Frame Depth: a `--max-depth`/env value if it parses, else
/// [`kernel::DEFAULT_MAX_DEPTH`]. Like [`resolve_keep`], an unparseable override
/// falls back to the default rather than failing the Run. The cap is at least 1:
/// the root Frame is Depth 1, so a 0 cap could never run even a flat Workflow.
pub fn resolve_max_depth(flag: Option<&str>, env: Option<&str>) -> u32 {
    flag.or(env)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_DEPTH)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::{resolve_keep, resolve_max_depth, runs_dir, DEFAULT_KEEP};
    use kernel::DEFAULT_MAX_DEPTH;
    use std::path::PathBuf;

    #[test]
    fn override_dir_wins_verbatim() {
        let got = runs_dir(Some("/tmp/custom"), Some("/xdg"), Some("/home"));
        assert_eq!(got, PathBuf::from("/tmp/custom"));
    }

    #[test]
    fn xdg_state_home_used_when_no_override() {
        let got = runs_dir(None, Some("/xdg"), Some("/home"));
        assert_eq!(got, PathBuf::from("/xdg/automedon/runs"));
    }

    #[test]
    fn home_default_used_when_no_xdg() {
        let got = runs_dir(None, None, Some("/home/u"));
        assert_eq!(got, PathBuf::from("/home/u/.local/state/automedon/runs"));
    }

    #[test]
    fn run_scratch_dir_is_under_the_temp_root_keyed_by_run_id() {
        let got = super::run_scratch_dir(&PathBuf::from("/tmp"), "01234567-run");
        assert_eq!(got, PathBuf::from("/tmp/automedon/runs/01234567-run"));
    }

    #[test]
    fn keep_prefers_flag_then_env_then_default() {
        assert_eq!(resolve_keep(Some("5"), Some("9")), 5);
        assert_eq!(resolve_keep(None, Some("9")), 9);
        assert_eq!(resolve_keep(None, None), DEFAULT_KEEP);
    }

    #[test]
    fn unparseable_keep_falls_back_to_default() {
        assert_eq!(resolve_keep(Some("lots"), None), DEFAULT_KEEP);
    }

    #[test]
    fn keep_is_clamped_to_at_least_one() {
        // 0 would otherwise prune the active Run's own directory (it is the
        // newest entry, and pruning keeps the newest `keep`).
        assert_eq!(resolve_keep(Some("0"), None), 1);
        assert_eq!(resolve_keep(None, Some("0")), 1);
    }

    #[test]
    fn max_depth_prefers_flag_then_env_then_default() {
        assert_eq!(resolve_max_depth(Some("3"), Some("9")), 3);
        assert_eq!(resolve_max_depth(None, Some("9")), 9);
        assert_eq!(resolve_max_depth(None, None), DEFAULT_MAX_DEPTH);
    }

    #[test]
    fn unparseable_max_depth_falls_back_to_default() {
        assert_eq!(resolve_max_depth(Some("deep"), None), DEFAULT_MAX_DEPTH);
    }

    #[test]
    fn max_depth_is_clamped_to_at_least_one() {
        // The root Frame is Depth 1; a 0 cap would reject every Workflow.
        assert_eq!(resolve_max_depth(Some("0"), None), 1);
    }
}
