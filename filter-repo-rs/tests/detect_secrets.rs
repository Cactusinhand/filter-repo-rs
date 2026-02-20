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

#[test]
fn detect_secrets_supports_custom_detect_pattern() {
    let repo = init_repo();

    write_file(&repo, "custom.txt", "internal=ZZZ-CUSTOM-SECRET-2026\n");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "add custom secret token"]);

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--detect-pattern")
        .arg(r"ZZZ-CUSTOM-SECRET-[0-9]{4}")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets with custom pattern");

    assert!(
        output.status.success(),
        "detect-secrets custom pattern should succeed"
    );

    let rules = repo.join("detected-secrets.txt");
    assert!(
        rules.exists(),
        "detected-secrets.txt should be generated for custom pattern"
    );
    let content = std::fs::read_to_string(&rules).expect("read detected-secrets.txt");
    assert!(
        content.contains("ZZZ-CUSTOM-SECRET-2026==>***REMOVED***"),
        "draft should include custom-pattern match: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_openai_api_key() {
    let repo = init_repo();

    write_file(
        &repo,
        "config.py",
        "OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz1234567890ABC\n",
    );
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "add openai key"]);

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets mode");

    assert!(output.status.success(), "detect-secrets should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "expected detection summary in stdout: {}",
        stdout
    );

    let rules = repo.join("detected-secrets.txt");
    let content = std::fs::read_to_string(&rules).expect("read detected-secrets.txt");
    assert!(
        content.contains("sk-abcdefghijklmnopqrstuvwxyz1234567890ABC==>***REMOVED***"),
        "draft should include openai api key rule: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_additional_common_patterns() {
    let repo = init_repo();
    let slack_domain = ["hooks", "slack", "com"].join(".");
    let slack_webhook = format!(
        "https://{}/services/T12345678/B12345678/abcdefghijklmnopqrstuvwx",
        slack_domain
    );
    let stripe_secret = ["sk", "live", "abcdefghijklmnopqrstuvwxyz123456"].join("_");
    let tokens_env = format!(
        "AWS_SECRET_ACCESS_KEY=abcdEFGHijklMNOPqrstUVWXyz0123456789+/AB\n\
GOOGLE_API_KEY=AIza12345678901234567890123456789012345\n\
GITLAB_TOKEN=glpat-abcDEF0123456789uvwxyzABCD\n\
NPM_TOKEN=npm_1234567890abcdefghijklmnopqrstuvwxyz\n\
SLACK_WEBHOOK={}\n\
STRIPE_SECRET={}\n",
        slack_webhook, stripe_secret
    );

    write_file(&repo, "tokens.env", &tokens_env);
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "add additional tokens"]);

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets mode");

    assert!(output.status.success(), "detect-secrets should succeed");

    let rules = repo.join("detected-secrets.txt");
    let content = std::fs::read_to_string(&rules).expect("read detected-secrets.txt");
    assert!(
        content.contains("abcdEFGHijklMNOPqrstUVWXyz0123456789+/AB==>***REMOVED***"),
        "draft should include aws secret access key: {}",
        content
    );
    assert!(
        content.contains("AIza12345678901234567890123456789012345==>***REMOVED***"),
        "draft should include google api key: {}",
        content
    );
    assert!(
        content.contains("glpat-abcDEF0123456789uvwxyzABCD==>***REMOVED***"),
        "draft should include gitlab pat: {}",
        content
    );
    assert!(
        content.contains("npm_1234567890abcdefghijklmnopqrstuvwxyz==>***REMOVED***"),
        "draft should include npm token: {}",
        content
    );
    assert!(
        content.contains(&format!("{}==>***REMOVED***", slack_webhook)),
        "draft should include slack webhook url: {}",
        content
    );
    assert!(
        content.contains(&format!("{}==>***REMOVED***", stripe_secret)),
        "draft should include stripe secret key: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_llm_vendor_keys() {
    let repo = init_repo();

    write_file(
        &repo,
        "llm.env",
        "GEMINI_API_KEY=AIza12345678901234567890123456789012345\n\
ANTHROPIC_API_KEY=sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab\n\
XAI_API_KEY=xai-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab\n\
DEEPSEEK_API_KEY=deepseek_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n\
GLM_API_KEY=zai-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab\n\
MINIMAX_API_KEY=minimax_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n\
KIMI_API_KEY=moonshot_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n\
QWEN_API_KEY=qwen_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n",
    );
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-q", "-m", "add llm keys"]);

    let output = cli_command()
        .arg("--detect-secrets")
        .arg("--dry-run")
        .current_dir(&repo)
        .output()
        .expect("run detect-secrets mode");

    assert!(output.status.success(), "detect-secrets should succeed");

    let rules = repo.join("detected-secrets.txt");
    let content = std::fs::read_to_string(&rules).expect("read detected-secrets.txt");
    assert!(
        content.contains("AIza12345678901234567890123456789012345==>***REMOVED***"),
        "draft should include gemini/google api key: {}",
        content
    );
    assert!(
        content.contains("sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab==>***REMOVED***"),
        "draft should include anthropic key: {}",
        content
    );
    assert!(
        content.contains("xai-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab==>***REMOVED***"),
        "draft should include xai key: {}",
        content
    );
    assert!(
        content.contains("deepseek_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789==>***REMOVED***"),
        "draft should include deepseek key: {}",
        content
    );
    assert!(
        content.contains("zai-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab==>***REMOVED***"),
        "draft should include glm(z.ai) key: {}",
        content
    );
    assert!(
        content.contains("minimax_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789==>***REMOVED***"),
        "draft should include minimax key: {}",
        content
    );
    assert!(
        content.contains("moonshot_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789==>***REMOVED***"),
        "draft should include kimi key: {}",
        content
    );
    assert!(
        content.contains("qwen_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789==>***REMOVED***"),
        "draft should include qwen key: {}",
        content
    );
}
