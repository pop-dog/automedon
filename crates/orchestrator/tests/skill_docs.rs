//! Guards the bundled skill docs against this PR's two breaking changes: the
//! binary is now `automedon` (not `orchestrator`), and the source/dev install
//! that rebuilds the engine moved from `./install.sh` to
//! `scripts/dev-install.sh`. A user who follows these skills verbatim must run
//! the right command and rebuild from the right script, so the docs are part of
//! the contract this PR changes.

use std::path::PathBuf;

fn skill(name: &str) -> String {
    let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join("skills")
        .join(name)
        .join("SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn skills_invoke_the_renamed_binary() {
    // The autocoder runs the coder Workflow; the engine skill shows the generic
    // invocation. Both must call the installed command, which is now `automedon`.
    let autocoder = skill("autocoder");
    assert!(
        autocoder.contains("automedon examples/coder.yaml"),
        "autocoder must run the coder Workflow with `automedon`"
    );

    let engine = skill("agent-orchestrator");
    assert!(
        engine.contains("automedon <workflow.yaml>"),
        "agent-orchestrator must show the `automedon` invocation"
    );

    // The removed command name must not survive as a runnable invocation;
    // following it verbatim yields `command not found`.
    for (name, body) in [("autocoder", &autocoder), ("agent-orchestrator", &engine)] {
        assert!(
            !body.contains("orchestrator examples/"),
            "{name} still invokes the removed `orchestrator` command"
        );
        assert!(
            !body.contains("| orchestrator"),
            "{name} still pipes into the removed `orchestrator` command"
        );
        assert!(
            !body.contains("orchestrator <workflow.yaml>"),
            "{name} still documents the removed `orchestrator` invocation"
        );
    }
}

#[test]
fn skills_rebuild_the_engine_with_the_dev_install_script() {
    // `install.sh` now downloads a prebuilt release; rebuilding the engine from
    // source is `scripts/dev-install.sh`. The skills' "re-install after engine
    // changes" guidance must point there, never at `./install.sh`.
    for name in ["autocoder", "agent-orchestrator"] {
        let body = skill(name);
        assert!(
            body.contains("scripts/dev-install.sh"),
            "{name} must rebuild the engine via scripts/dev-install.sh"
        );
        assert!(
            !body.contains("./install.sh"),
            "{name} still tells the user to re-run ./install.sh, which no longer \
             builds from source"
        );
    }
}
