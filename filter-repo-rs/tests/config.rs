use std::process::Command;

mod common;
use common::*;

fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_filter-repo-rs"))
}

#[test]
fn docs_example_config_requires_debug_mode() {
    let repo = init_repo();
    let config_path = docs_example_config_path();

    let output = cli_command()
        .current_dir(&repo)
        .arg("--config")
        .arg(&config_path)
        .arg("--analyze")
        .output()
        .expect("run filter-repo-rs with docs config");

    assert_eq!(
        Some(2),
        output.status.code(),
        "config thresholds should be gated behind debug mode"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("gated behind debug mode"),
        "expected gating message in stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("cli-convergence"),
        "expected docs pointer in gating message: {}",
        stderr
    );
}

#[test]
fn docs_example_config_runs_under_debug_mode() {
    let repo = init_repo();
    let config_path = docs_example_config_path();

    let output = cli_command()
        .current_dir(&repo)
        .arg("--debug-mode")
        .arg("--config")
        .arg(&config_path)
        .arg("--analyze")
        .output()
        .expect("run filter-repo-rs analyze with docs config");

    assert!(
        output.status.success(),
        "analyze run with docs config should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Repository analysis"),
        "expected human analysis output when using docs config: {}",
        stdout
    );
    assert!(
        stdout.contains("Total size"),
        "expected total size summary in analysis output: {}",
        stdout
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("gated behind debug mode"),
        "debug mode should prevent gating message: {}",
        stderr
    );
}
