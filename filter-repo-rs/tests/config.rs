mod common;
use common::*;

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

#[test]
fn cli_arguments_override_repo_config() {
    let repo = init_repo();
    write_file(
        &repo,
        ".filter-repo-rs.toml",
        "[analyze]\njson = false\ntop = 4\n",
    );
    for idx in 0..5 {
        let file_path = format!("blob-{idx}.bin");
        let payload = vec![b'a' + (idx as u8); 1024 + (idx * 10)];
        std::fs::write(repo.join(&file_path), &payload)
            .unwrap_or_else(|e| panic!("failed to write test blob {file_path}: {e}"));
        run_git(&repo, &["add", &file_path]);
    }
    run_git(&repo, &["commit", "-m", "add large blobs"]);

    let baseline = cli_command()
        .current_dir(&repo)
        .arg("--debug-mode")
        .arg("--analyze")
        .output()
        .expect("run analysis with repo config");
    assert!(
        baseline.status.success(),
        "analysis run with config should succeed"
    );
    let stdout_baseline = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        stdout_baseline.contains("Top 4 blobs by size"),
        "config-defined top should appear in baseline output: {}",
        stdout_baseline
    );

    let override_out = cli_command()
        .current_dir(&repo)
        .arg("--debug-mode")
        .arg("--analyze")
        .arg("--analyze-top")
        .arg("2")
        .output()
        .expect("run analysis with CLI override");
    assert!(
        override_out.status.success(),
        "analysis run with CLI override should succeed"
    );
    let stdout_override = String::from_utf8_lossy(&override_out.stdout);
    assert!(
        stdout_override.contains("Top 2 blobs by size"),
        "CLI --analyze-top should override config top: {}",
        stdout_override
    );
    assert!(
        !stdout_override.contains("Top 4 blobs by size"),
        "override output should no longer mention config top value"
    );
}

#[test]
fn invalid_repo_config_emits_friendly_error() {
    let repo = init_repo();
    write_file(
        &repo,
        ".filter-repo-rs.toml",
        "[analyze\nthis is not valid toml",
    );

    let output = cli_command()
        .current_dir(&repo)
        .arg("--analyze")
        .output()
        .expect("run analysis with invalid config");

    assert_eq!(
        Some(2),
        output.status.code(),
        "invalid config should cause CLI failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to parse config"),
        "parse failure should mention config error: {}",
        stderr
    );
    assert!(
        stderr.contains("cli-convergence"),
        "parse failure should point to CLI convergence docs: {}",
        stderr
    );
}
