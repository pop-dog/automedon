//! Maps a loader Workflow id (`<canonical file path>#<name>`, see
//! `loader::make_id`) to a display id safe for output: an invoked root file's
//! own Workflows render as their bare name, and cross-file Workflows render as
//! the referenced path relative to the root file's directory, plus the name.
//! Absolute paths never reach `graph` or `run --dry-run` output through this
//! helper — the loader's ids are load-time plumbing, not display text.

use std::path::{Path, PathBuf};

/// Split a loader Workflow id into its file path and Workflow name at the
/// last `#`, mirroring `loader::make_id`'s `<path>#<name>` format. An id with
/// no `#` (a hand-built test fixture registry, never one `loader::load`
/// produces) has no file component, so it round-trips as its own display id.
fn split(id: &str) -> (&str, &str) {
    id.rsplit_once('#').unwrap_or(("", id))
}

/// The display id for Workflow `id`, given the registry's `root` id (whose
/// file anchors "same file as the invocation" and the base for relative
/// paths to every other file).
pub fn display_id(id: &str, root: &str) -> String {
    let (file, name) = split(id);
    let (root_file, _) = split(root);
    if file == root_file {
        return name.to_string();
    }
    let root_dir = Path::new(root_file).parent().unwrap_or_else(|| Path::new(""));
    let rel = relative_to(Path::new(file), root_dir);
    format!("{}#{name}", rel.display())
}

/// `target`'s path relative to `base`, assuming both are absolute and share a
/// common ancestor (true of any two canonicalized paths on the same
/// filesystem root) — walk up from `base` past the shared prefix, then back
/// down to `target`.
fn relative_to(target: &Path, base: &Path) -> PathBuf {
    let target_parts: Vec<_> = target.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let common = target_parts.iter().zip(base_parts.iter()).take_while(|(a, b)| a == b).count();

    let mut rel = PathBuf::new();
    for _ in common..base_parts.len() {
        rel.push("..");
    }
    for part in &target_parts[common..] {
        rel.push(part.as_os_str());
    }
    rel
}

#[cfg(test)]
mod tests {
    use super::display_id;

    #[test]
    fn a_root_file_workflow_displays_as_its_bare_name() {
        let root = "/home/user/project/coder.yaml#coder";
        assert_eq!(display_id("/home/user/project/coder.yaml#develop", root), "develop");
    }

    #[test]
    fn a_cross_file_workflow_displays_as_a_relative_path_and_name() {
        let root = "/home/user/project/autocoder.yaml#autocoder";
        assert_eq!(
            display_id("/home/user/project/coder.yaml#coder", root),
            "coder.yaml#coder"
        );
    }

    #[test]
    fn a_cross_file_workflow_in_a_subdirectory_displays_with_its_relative_path() {
        let root = "/home/user/project/autocoder.yaml#autocoder";
        assert_eq!(
            display_id("/home/user/project/lib/coder.yaml#coder", root),
            "lib/coder.yaml#coder"
        );
    }

    #[test]
    fn a_cross_file_workflow_in_a_sibling_directory_walks_up_and_back_down() {
        let root = "/home/user/project/mid/parent.yaml#main";
        assert_eq!(
            display_id("/home/user/project/leaf/child.yaml#leaf", root),
            "../leaf/child.yaml#leaf"
        );
    }

    #[test]
    fn no_absolute_path_ever_survives_into_a_display_id() {
        let root = "/home/user/project/autocoder.yaml#autocoder";
        let displayed = display_id("/home/user/project/lib/coder.yaml#coder", root);
        assert!(!displayed.starts_with('/'), "{displayed}");
    }
}
