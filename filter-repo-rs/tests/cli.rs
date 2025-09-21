use std::process::Command;

mod common;
use common::*;

fn cli_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_filter-repo-rs"))
}

#[test]
fn help_hides_debug_sections_without_debug_mode() {
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
    assert!(
        !stdout.contains("Debug / fast-export passthrough"),
        "baseline help should hide fast-export passthrough header"
    );
    assert!(
        !stdout.contains("--no-reencode"),
        "baseline help should hide fast-export passthrough flags"
    );
    assert!(
        !stdout.contains("--date-order"),
        "baseline help should hide date-order toggle"
    );
}

#[test]
fn help_shows_debug_sections_in_debug_mode() {
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
    assert!(
        stdout.contains("Debug / fast-export passthrough"),
        "debug help should list fast-export passthrough header"
    );
    assert!(
        stdout.contains("--no-reencode"),
        "debug help should list fast-export passthrough flags"
    );
    assert!(
        stdout.contains("--date-order"),
        "debug help should also list date-order toggle"
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
fn fast_export_debug_flags_require_debug_mode() {
    let gated_flags = [
        "--date-order",
        "--no-reencode",
        "--no-quotepath",
        "--no-mark-tags",
        "--mark-tags",
    ];

    for flag in gated_flags {
        let output = cli_command()
            .arg(flag)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "failed to run filter-repo-rs with gated flag {}: {}",
                    flag, e
                )
            });

        assert_eq!(
            Some(2),
            output.status.code(),
            "gated fast-export flag '{}' should exit with code 2",
            flag
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("FRRS_DEBUG"),
            "gated message for flag '{}' should mention FRRS_DEBUG",
            flag
        );
    }
}

#[test]
fn debug_mode_allows_fast_export_debug_flags() {
    let gated_flags = [
        "--date-order",
        "--no-reencode",
        "--no-quotepath",
        "--no-mark-tags",
        "--mark-tags",
    ];

    for flag in gated_flags {
        let output = cli_command()
            .arg("--debug-mode")
            .arg(flag)
            .arg("--help")
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "failed to run filter-repo-rs --debug-mode with flag {}: {}",
                    flag, e
                )
            });

        assert!(
            output.status.success(),
            "debug mode should allow fast-export flag '{}'",
            flag
        );
    }
}

#[test]
fn debug_mode_allows_analysis_threshold_flags() {
    let repo = init_repo();
    let output = cli_command()
        .arg("--debug-mode")
        .arg("--date-order")
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
