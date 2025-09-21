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
    assert!(
        !stdout.contains("--no-reset"),
        "baseline help should hide cleanup debug flag"
    );
    assert!(
        !stdout.contains("--cleanup-aggressive"),
        "baseline help should hide aggressive cleanup toggle"
    );
    assert!(
        !stdout.contains("--fe_stream_override"),
        "baseline help should hide stream override"
    );
    assert!(
        !stdout.contains("Debug / cleanup behavior"),
        "baseline help should hide cleanup debug section"
    );
    assert!(
        !stdout.contains("Debug / stream overrides"),
        "baseline help should hide stream override section"
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
        stdout.contains("Debug / analysis thresholds"),
        "debug section header missing"
    );
    assert!(
        stdout.contains("Configure analyze.thresholds.* via"),
        "debug help should mention config guidance for thresholds"
    );
    assert!(
        stdout.contains("Legacy --analyze-*-warn CLI flags remain for compatibility"),
        "debug help should include legacy compatibility note"
    );
    assert!(
        !stdout.contains("--analyze-total-warn"),
        "debug help should omit direct legacy flag listing"
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
    assert!(
        stdout.contains("Debug / cleanup behavior"),
        "debug help should list cleanup debug section"
    );
    assert!(
        stdout.contains("--no-reset"),
        "debug help should surface no-reset flag"
    );
    assert!(
        stdout.contains("--cleanup-aggressive"),
        "debug help should list cleanup-aggressive flag"
    );
    assert!(
        stdout.contains("Debug / stream overrides"),
        "debug help should list stream override section"
    );
    assert!(
        stdout.contains("--fe_stream_override"),
        "debug help should list stream override flag"
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
fn debug_only_flags_require_debug_mode() {
    let gated_cases: &[(&[&str], &str)] = &[
        (&["--date-order"], "--date-order"),
        (&["--no-reencode"], "--no-reencode"),
        (&["--no-quotepath"], "--no-quotepath"),
        (&["--no-mark-tags"], "--no-mark-tags"),
        (&["--mark-tags"], "--mark-tags"),
        (&["--no-reset"], "--no-reset"),
        (&["--cleanup-aggressive"], "--cleanup-aggressive"),
        (&["--fe_stream_override", "dummy"], "--fe_stream_override"),
    ];

    for &(args, flag) in gated_cases {
        let output = cli_command().args(args).output().unwrap_or_else(|e| {
            panic!(
                "failed to run filter-repo-rs with gated flag {}: {}",
                flag, e
            )
        });

        assert_eq!(
            Some(2),
            output.status.code(),
            "gated flag '{}' should exit with code 2",
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
fn debug_mode_allows_debug_only_flags() {
    let gated_cases: &[(&[&str], &str)] = &[
        (&["--date-order"], "--date-order"),
        (&["--no-reencode"], "--no-reencode"),
        (&["--no-quotepath"], "--no-quotepath"),
        (&["--no-mark-tags"], "--no-mark-tags"),
        (&["--mark-tags"], "--mark-tags"),
        (&["--no-reset"], "--no-reset"),
        (&["--cleanup-aggressive"], "--cleanup-aggressive"),
        (&["--fe_stream_override", "dummy"], "--fe_stream_override"),
    ];

    for &(args, flag) in gated_cases {
        let output = cli_command()
            .arg("--debug-mode")
            .args(args)
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
            "debug mode should allow flag '{}'",
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

#[test]
fn cleanup_flag_supports_new_and_legacy_syntax() {
    let repo = init_repo();

    let legacy_eq = cli_command()
        .arg("--cleanup=standard")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run filter-repo-rs with legacy cleanup syntax (--cleanup=standard)");

    assert!(
        legacy_eq.status.success(),
        "legacy cleanup syntax should still run"
    );
    let stderr_eq = String::from_utf8_lossy(&legacy_eq.stderr);
    assert!(
        stderr_eq.contains("deprecated"),
        "legacy --cleanup= mode should emit deprecation warning: {}",
        stderr_eq
    );
    assert!(
        stderr_eq.contains("--cleanup"),
        "deprecation warning should mention --cleanup guidance: {}",
        stderr_eq
    );

    let legacy_split = cli_command()
        .arg("--cleanup")
        .arg("none")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run filter-repo-rs with legacy cleanup syntax (--cleanup none)");

    assert!(
        legacy_split.status.success(),
        "legacy split cleanup syntax should run"
    );
    let stderr_split = String::from_utf8_lossy(&legacy_split.stderr);
    assert!(
        stderr_split.contains("deprecated"),
        "legacy split syntax should emit deprecation warning: {}",
        stderr_split
    );

    let legacy_agg = cli_command()
        .arg("--debug-mode")
        .arg("--cleanup=aggressive")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run filter-repo-rs with legacy cleanup syntax (--cleanup=aggressive)");

    assert!(
        legacy_agg.status.success(),
        "legacy aggressive cleanup syntax should run"
    );
    let stderr_agg = String::from_utf8_lossy(&legacy_agg.stderr);
    assert!(
        stderr_agg.contains("deprecated"),
        "legacy --cleanup=aggressive mode should emit deprecation warning: {}",
        stderr_agg
    );
    assert!(
        stderr_agg.contains("--cleanup-aggressive"),
        "deprecation warning for aggressive should mention --cleanup-aggressive: {}",
        stderr_agg
    );

    let new_flag = cli_command()
        .arg("--cleanup")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run filter-repo-rs with boolean --cleanup");

    assert!(
        new_flag.status.success(),
        "boolean --cleanup should succeed"
    );
    let stderr_new = String::from_utf8_lossy(&new_flag.stderr);
    assert!(
        !stderr_new.contains("deprecated"),
        "boolean --cleanup should not emit deprecation warning: {}",
        stderr_new
    );
}
