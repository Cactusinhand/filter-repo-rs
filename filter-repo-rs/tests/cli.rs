use std::process::Command;

mod common;
use common::*;

fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_filter-repo-rs"))
}

#[test]
fn help_hides_analysis_thresholds_without_debug() {
    let output = cli_command()
        .arg("--help")
        .output()
        .expect("run filter-repo-rs --help");

    assert!(output.status.success(), "help should exit successfully");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--analyze-top"),
        "baseline help should mention analyze-top"
    );
    assert!(
        stdout.contains("--debug-mode"),
        "baseline help should mention debug-mode toggle"
    );
    assert!(
        !stdout.contains("--analyze-total-warn"),
        "baseline help should hide threshold overrides"
    );
}

#[test]
fn help_shows_analysis_thresholds_in_debug_mode() {
    let output = cli_command()
        .arg("--debug-mode")
        .arg("--help")
        .output()
        .expect("run filter-repo-rs --debug-mode --help");

    assert!(
        output.status.success(),
        "debug help should exit successfully"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--analyze-total-warn"),
        "debug help should list threshold flag"
    );
    assert!(
        stdout.contains("Debug / analysis thresholds"),
        "debug section header missing"
    );
}

#[test]
fn analysis_threshold_flags_require_debug_mode() {
    let output = cli_command()
        .arg("--analyze-total-warn")
        .arg("1")
        .output()
        .expect("run filter-repo-rs with gated flag");

    assert_eq!(
        Some(2),
        output.status.code(),
        "gated flag should exit with code 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FRRS_DEBUG"),
        "gated message should mention FRRS_DEBUG"
    );
}

#[test]
fn debug_mode_allows_analysis_threshold_flags() {
    let repo = init_repo();
    let output = cli_command()
        .arg("--debug-mode")
        .arg("--analyze")
        .arg("--analyze-total-warn")
        .arg("1")
        .current_dir(&repo)
        .output()
        .expect("run filter-repo-rs analyze in debug mode");

    assert!(
        output.status.success(),
        "debug mode should allow threshold overrides"
    );
}
