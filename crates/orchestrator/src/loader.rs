//! The path-based `WorkflowSource`: given a root file, transitively load every
//! referenced file and assemble a single kernel [`Registry`]. The
//! Kernel still addresses sub-Workflows by id; this loader canonicalizes paths to
//! ids before the Kernel ever sees them, so `{ path: … }` never reaches the
//! engine and the Kernel stays the one schema source of truth.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use kernel::Registry;
use serde_yaml::{Mapping, Value};

type LoadError = Box<dyn std::error::Error>;

/// Load the Registry rooted at `root`, transitively merging every referenced
/// file. See module docs for the resolution strategy.
pub fn load(root: &Path) -> Result<Registry, LoadError> {
    let root = canonicalize(root)?;
    let mut loader = Loader::default();
    loader.merge_file(&root)?;
    let root_id = make_id(&root, &loader.root_name(&root)?);
    loader.validate_references()?;

    let mut registry = Mapping::new();
    registry.insert(Value::from("root"), Value::from(root_id));
    registry.insert(Value::from("workflows"), Value::Mapping(loader.workflows));
    Ok(serde_yaml::from_value(Value::Mapping(registry))?)
}

/// Accumulates the merged registry as files are loaded. Files are memoized by
/// canonical path: `parsed` caches each file's `serde_yaml::Value` so it is read
/// and parsed once, and `merged` records which files' Workflows are already in
/// `workflows` so a self- or mutual cycle loads once instead of recursing forever.
#[derive(Default)]
struct Loader {
    /// Canonical file path -> the file's parsed `Value`.
    parsed: HashMap<PathBuf, Value>,
    /// Canonical file path -> already folded into `workflows`.
    merged: HashSet<PathBuf>,
    /// Every Workflow keyed by its namespaced id (`<canonical path>#<name>`).
    workflows: Mapping,
}

impl Loader {
    /// Fold one file's Workflows into the registry under namespaced ids, then
    /// recurse into the files it references. Idempotent per canonical path: a file
    /// already merged returns immediately, which is what bounds load-time cycles.
    fn merge_file(&mut self, path: &Path) -> Result<(), LoadError> {
        if !self.merged.insert(path.to_path_buf()) {
            return Ok(());
        }
        let file = self.parse(path)?.clone();
        let workflows = file
            .get("workflows")
            .and_then(Value::as_mapping)
            .ok_or_else(|| format!("{}: missing top-level `workflows`", path.display()))?;

        // Rewrite each Workflow's references to namespaced ids before merging,
        // collecting the referenced files to load once this one is folded in.
        let mut referenced = Vec::new();
        for (name, workflow) in workflows {
            let name = name
                .as_str()
                .ok_or_else(|| format!("{}: workflow name must be a string", path.display()))?;
            let id = make_id(path, name);
            let rewritten = self.rewrite_workflow(workflow.clone(), path, &mut referenced)?;
            self.workflows.insert(Value::from(id), rewritten);
        }
        for target in referenced {
            self.merge_file(&target)?;
        }
        Ok(())
    }

    /// Rewrite every Composite Step's `workflow:` reference to the namespaced id it
    /// resolves to: a by-name reference binds within `path`; a `{ path: P }`
    /// reference binds to the target file's root Workflow (P resolved relative to
    /// `path`'s directory). Referenced files are pushed onto `referenced`.
    fn rewrite_workflow(
        &mut self,
        mut workflow: Value,
        path: &Path,
        referenced: &mut Vec<PathBuf>,
    ) -> Result<Value, LoadError> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        if let Some(Value::Mapping(steps)) = workflow.get_mut("steps") {
            for (_, step) in steps.iter_mut() {
                let Value::Mapping(step) = step else { continue };
                let Some(body) = step.get("workflow").cloned() else { continue };
                let id = match body {
                    // A by-name reference is local to the referring file.
                    Value::String(name) => make_id(path, &name),
                    // A `{ path: P }` reference binds to the target file's root.
                    Value::Mapping(map) => {
                        let p = map.get("path").and_then(Value::as_str).ok_or_else(|| {
                            format!("{}: `workflow` map must have a `path`", path.display())
                        })?;
                        let target = canonicalize(&dir.join(p))?;
                        let root = self.root_name(&target)?;
                        referenced.push(target.clone());
                        make_id(&target, &root)
                    }
                    other => {
                        return Err(format!(
                            "{}: `workflow` must be a name or {{ path: … }}, got {other:?}",
                            path.display()
                        )
                        .into())
                    }
                };
                step.insert(Value::from("workflow"), Value::from(id));
            }
        }
        Ok(workflow)
    }

    /// The name of a file's root Workflow, verified to exist in its `workflows:`
    /// map. An absent root is a load error so a dangling `{ path }` reference is
    /// caught here rather than as a mysterious missing id at run time.
    fn root_name(&mut self, path: &Path) -> Result<String, LoadError> {
        let file = self.parse(path)?;
        let root = file
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{}: missing top-level `root`", path.display()))?
            .to_string();
        let present = file
            .get("workflows")
            .and_then(|w| w.get(&root))
            .is_some();
        if !present {
            return Err(format!("{}: root workflow `{root}` is absent", path.display()).into());
        }
        Ok(root)
    }

    /// Verify every rewritten Composite `workflow` id names a Workflow that was
    /// actually merged. A by-name reference resolves to `<this file>#<name>`
    /// unconditionally, so a typo (`workflow: revieweer`) would otherwise reach the
    /// Kernel as a missing id and panic at run time; catching it here turns it into
    /// a load error naming the dangling id (which carries the file and name).
    fn validate_references(&self) -> Result<(), LoadError> {
        for workflow in self.workflows.values() {
            let Some(Value::Mapping(steps)) = workflow.get("steps") else {
                continue;
            };
            for (_, step) in steps {
                let Some(Value::String(id)) = step.get("workflow") else {
                    continue;
                };
                if !self.workflows.contains_key(id.as_str()) {
                    return Err(format!("references missing workflow `{id}`").into());
                }
            }
        }
        Ok(())
    }

    /// Read and parse a file once, caching its `Value` by canonical path.
    fn parse(&mut self, path: &Path) -> Result<&Value, LoadError> {
        if !self.parsed.contains_key(path) {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let value = serde_yaml::from_str(&text)
                .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
            self.parsed.insert(path.to_path_buf(), value);
        }
        Ok(&self.parsed[path])
    }
}

/// The registry id for a Workflow: its file's canonical absolute path joined to
/// the Workflow's name, so ids from different files never collide.
fn make_id(canonical: &Path, name: &str) -> String {
    format!("{}#{}", canonical.display(), name)
}

/// Canonicalize a path to an absolute one, reporting the offending path on a
/// missing or unreadable file.
fn canonicalize(path: &Path) -> Result<PathBuf, LoadError> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("cannot resolve workflow file {}: {e}", path.display()).into())
}

#[cfg(test)]
mod tests {
    use super::load;
    use kernel::StepBody;
    use std::path::{Path, PathBuf};

    /// A throwaway directory under the system temp dir, removed on Drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("ao-load-{tag}-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn write(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, body).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The canonical id a loaded file's Workflow gets: `<abs path>#<name>`.
    fn id(path: &Path, name: &str) -> String {
        format!("{}#{}", std::fs::canonicalize(path).unwrap().display(), name)
    }

    #[test]
    fn single_file_ids_are_namespaced_by_canonical_path() {
        // A within-file Composite reference (`workflow: child`) is rewritten to the
        // file's namespaced id, and the registry root is the root-Workflow's id.
        let dir = TempDir::new("single");
        let wf = dir.write(
            "wf.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: child
        gates:
          - { key: 0, target: { exit: 0 } }
  child:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let reg = load(&wf).unwrap();
        assert_eq!(reg.root, id(&wf, "main"));
        assert!(reg.workflows.contains_key(&id(&wf, "main")));
        assert!(reg.workflows.contains_key(&id(&wf, "child")));
        match &reg.workflows[&id(&wf, "main")].steps["call"].body {
            StepBody::Workflow(target) => assert_eq!(*target, id(&wf, "child")),
            other => panic!("expected a Composite Step, got {other:?}"),
        }
    }

    #[test]
    fn path_reference_binds_to_the_target_files_root_workflow() {
        // A `{ path: ./child.yaml }` reference binds to that file's root Workflow,
        // namespaced by the child file's canonical path — and the child's own
        // Workflows are merged into the same registry.
        let dir = TempDir::new("xfile");
        let child = dir.write(
            "child.yaml",
            r#"
root: reviewer
workflows:
  reviewer:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let parent = dir.write(
            "parent.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: { path: ./child.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let reg = load(&parent).unwrap();
        assert_eq!(reg.root, id(&parent, "main"));
        assert!(reg.workflows.contains_key(&id(&child, "reviewer")));
        match &reg.workflows[&id(&parent, "main")].steps["call"].body {
            StepBody::Workflow(target) => assert_eq!(*target, id(&child, "reviewer")),
            other => panic!("expected a Composite Step, got {other:?}"),
        }
    }

    #[test]
    fn path_is_resolved_relative_to_the_referring_file() {
        // `mid/parent.yaml` references `../leaf/child.yaml`. Resolution must be
        // relative to the referring file's own directory (`mid/`), not the root
        // file's directory or the process cwd.
        let dir = TempDir::new("relative");
        std::fs::create_dir_all(dir.0.join("mid")).unwrap();
        std::fs::create_dir_all(dir.0.join("leaf")).unwrap();
        let child = dir.write(
            "leaf/child.yaml",
            r#"
root: leaf
workflows:
  leaf:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let mid = dir.write(
            "mid/parent.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: { path: ../leaf/child.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let reg = load(&mid).unwrap();
        match &reg.workflows[&id(&mid, "main")].steps["call"].body {
            StepBody::Workflow(target) => assert_eq!(*target, id(&child, "leaf")),
            other => panic!("expected a Composite Step, got {other:?}"),
        }
    }

    #[test]
    fn a_shared_child_referenced_by_two_parents_is_merged_once() {
        // `child.yaml` is referenced by two distinct parent files. It binds to a
        // single canonical id, so the registry holds it once — reuse, not a copy
        // per consumer.
        let dir = TempDir::new("reuse");
        let child = dir.write(
            "child.yaml",
            r#"
root: shared
workflows:
  shared:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        for name in ["a.yaml", "b.yaml"] {
            dir.write(
                name,
                r#"
root: parent
workflows:
  parent:
    entry: call
    steps:
      call:
        workflow: { path: ./child.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
            );
        }
        let root = dir.write(
            "root.yaml",
            r#"
root: top
workflows:
  top:
    entry: a
    steps:
      a:
        workflow: { path: ./a.yaml }
        gates:
          - { key: 0, target: { step: b } }
      b:
        workflow: { path: ./b.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let reg = load(&root).unwrap();
        // top + a's parent + b's parent + the shared child = four Workflows, with
        // the child present exactly once.
        assert_eq!(reg.workflows.len(), 4);
        assert!(reg.workflows.contains_key(&id(&child, "shared")));
    }

    #[test]
    fn a_self_referencing_file_loads_once_without_error() {
        // A file whose Composite Step references its own path must load (memoized
        // by canonical path) rather than recurse forever or error at load time.
        // Runtime recursion stays bounded by the Depth cap, not refused here.
        let dir = TempDir::new("cycle");
        let wf = dir.write(
            "deep.yaml",
            r#"
root: deep
workflows:
  deep:
    entry: recurse
    steps:
      recurse:
        workflow: { path: ./deep.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let reg = load(&wf).unwrap();
        assert_eq!(reg.workflows.len(), 1);
        match &reg.workflows[&id(&wf, "deep")].steps["recurse"].body {
            StepBody::Workflow(target) => assert_eq!(*target, id(&wf, "deep")),
            other => panic!("expected a self-referencing Composite Step, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_referenced_file_is_a_load_error_naming_the_path() {
        let dir = TempDir::new("missing");
        let parent = dir.write(
            "parent.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: { path: ./nope.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let err = load(&parent).unwrap_err().to_string();
        assert!(err.contains("nope.yaml"), "error should name the path: {err}");
    }

    #[test]
    fn a_dangling_by_name_reference_is_a_load_error_naming_the_target() {
        // A within-file `workflow: <name>` that names no Workflow in the file's
        // `workflows:` map cannot bind. It must be caught at load with the missing
        // name reported, not rewritten to an id that panics the Kernel at run time.
        let dir = TempDir::new("dangling");
        let wf = dir.write(
            "wf.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: revieweer
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let err = load(&wf).unwrap_err().to_string();
        assert!(
            err.contains("revieweer"),
            "error should name the missing workflow: {err}"
        );
    }

    #[test]
    fn a_path_to_a_file_with_an_absent_root_is_a_load_error() {
        // The child file's `root:` names a Workflow not in its `workflows:` map, so
        // the by-path reference cannot bind. This is caught at load with the
        // offending file named, not deferred to a missing id at run time.
        let dir = TempDir::new("absent-root");
        let child = dir.write(
            "child.yaml",
            r#"
root: ghost
workflows:
  present:
    entry: work
    steps:
      work:
        command: "exit 0"
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );
        let parent = dir.write(
            "parent.yaml",
            r#"
root: main
workflows:
  main:
    entry: call
    steps:
      call:
        workflow: { path: ./child.yaml }
        gates:
          - { key: 0, target: { exit: 0 } }
"#,
        );

        let err = load(&parent).unwrap_err().to_string();
        assert!(err.contains("ghost"), "error should name the absent root: {err}");
        assert!(
            err.contains(child.file_name().unwrap().to_str().unwrap()),
            "error should name the offending file: {err}"
        );
    }
}
