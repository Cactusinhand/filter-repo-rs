mod common;
use common::*;

#[test]
fn detect_secrets_dry_run_writes_draft_file() {
    let repo = init_repo();

    write_file(
        &repo,
        "app.env",
        "AWS_ACCESS_KEY_ID=AKIA1234567890ABCDEF\npassword=superSecret123\n",
    );
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "add secret-like values"]);

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets mode");

    assert!(
        output.status.success(),
        "detect-secrets dry-run should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "expected detection summary in stdout: {}",
        stdout
    );

    let rules = repo.join("detected-secrets.txt");
    assert!(rules.exists(), "detected-secrets.txt should be generated");
    let content = std::fs::read_to_string(&rules).expect("read detected-secrets.txt");
    assert!(
        content.contains("AKIA1234567890ABCDEF==>***REMOVED***"),
        "draft should include aws access key rule: {}",
        content
    );
}

#[test]
fn detect_secrets_reports_zero_when_no_matches() {
    let repo = init_repo();

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets mode on clean repo");

    assert!(
        output.status.success(),
        "detect-secrets on clean repo should succeed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 potential secrets"),
        "expected zero summary in stdout: {}",
        stdout
    );
}
