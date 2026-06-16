//! Resolving where Runs are logged and how many to retain. Kept as pure
//! functions (no env/argv access) so the precedence rules are unit-testable
//! (ADR-0009, decisions 4 and 7).

use std::path::PathBuf;

/// Application subdirectory under the chosen state root.
const APP_DIR: &str = "agent-orchestrator";

/// Default number of Runs to retain when no override is given.
pub const DEFAULT_KEEP: usize = 100;

/// Resolve the directory that holds per-Run subdirectories.
///
/// Precedence: an explicit `override_dir` (from `--log-dir`/env) is used
/// verbatim; otherwise `$XDG_STATE_HOME/agent-orchestrator/runs`; otherwise the
/// XDG default `~/.local/state/agent-orchestrator/runs`. With no home either, a
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

/// Resolve the retention cap: a `--keep`/env value if it parses, else
/// [`DEFAULT_KEEP`]. An unparseable override falls back to the default rather
/// than failing the Run.
pub fn resolve_keep(flag: Option<&str>, env: Option<&str>) -> usize {
    flag.or(env)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_KEEP)
}

#[cfg(test)]
mod tests {
    use super::{resolve_keep, runs_dir, DEFAULT_KEEP};
    use std::path::PathBuf;

    #[test]
    fn override_dir_wins_verbatim() {
        let got = runs_dir(Some("/tmp/custom"), Some("/xdg"), Some("/home"));
        assert_eq!(got, PathBuf::from("/tmp/custom"));
    }

    #[test]
    fn xdg_state_home_used_when_no_override() {
        let got = runs_dir(None, Some("/xdg"), Some("/home"));
        assert_eq!(got, PathBuf::from("/xdg/agent-orchestrator/runs"));
    }

    #[test]
    fn home_default_used_when_no_xdg() {
        let got = runs_dir(None, None, Some("/home/u"));
        assert_eq!(got, PathBuf::from("/home/u/.local/state/agent-orchestrator/runs"));
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
}
