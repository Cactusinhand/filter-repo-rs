mod common;
use common::fake_secrets;
use common::*;

#[test]
fn detect_secrets_dry_run_writes_draft_file() {
    let repo = init_repo();
    let aws_access_key_id = fake_secrets::aws_access_key_id();
    let super_secret = fake_secrets::super_secret_123();

    write_file(
        &repo,
        "app.env",
        &format!("AWS_ACCESS_KEY_ID={aws_access_key_id}\npassword={super_secret}\n"),
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
        content.contains(&fake_secrets::removed_rule(&aws_access_key_id)),
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
    let custom_secret = fake_secrets::custom_secret_2026();

    write_file(&repo, "custom.txt", &format!("internal={custom_secret}\n"));
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
        content.contains(&fake_secrets::removed_rule(&custom_secret)),
        "draft should include custom-pattern match: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_openai_api_key() {
    let repo = init_repo();
    let openai_api_key = fake_secrets::openai_api_key();

    write_file(
        &repo,
        "config.py",
        &format!("OPENAI_API_KEY={openai_api_key}\n"),
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
        content.contains(&fake_secrets::removed_rule(&openai_api_key)),
        "draft should include openai api key rule: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_additional_common_patterns() {
    let repo = init_repo();
    let aws_secret_access_key = fake_secrets::aws_secret_access_key();
    let google_api_key = fake_secrets::google_api_key();
    let gitlab_token = fake_secrets::gitlab_token();
    let npm_token = fake_secrets::npm_token();
    let slack_webhook = fake_secrets::slack_webhook_url();
    let stripe_secret = fake_secrets::stripe_live_secret();
    let tokens_env = format!(
        "AWS_SECRET_ACCESS_KEY={aws_secret_access_key}\n\
GOOGLE_API_KEY={google_api_key}\n\
GITLAB_TOKEN={gitlab_token}\n\
NPM_TOKEN={npm_token}\n\
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
        content.contains(&fake_secrets::removed_rule(&aws_secret_access_key)),
        "draft should include aws secret access key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&google_api_key)),
        "draft should include google api key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&gitlab_token)),
        "draft should include gitlab pat: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&npm_token)),
        "draft should include npm token: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&slack_webhook)),
        "draft should include slack webhook url: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&stripe_secret)),
        "draft should include stripe secret key: {}",
        content
    );
}

#[test]
fn detect_secrets_detects_llm_vendor_keys() {
    let repo = init_repo();
    let google_api_key = fake_secrets::google_api_key();
    let anthropic_api_key = fake_secrets::anthropic_api_key();
    let xai_api_key = fake_secrets::xai_api_key();
    let deepseek_api_key = fake_secrets::deepseek_api_key();
    let zai_api_key = fake_secrets::zai_api_key();
    let minimax_api_key = fake_secrets::minimax_api_key();
    let moonshot_api_key = fake_secrets::moonshot_api_key();
    let qwen_api_key = fake_secrets::qwen_api_key();

    write_file(
        &repo,
        "llm.env",
        &format!(
            "GEMINI_API_KEY={google_api_key}\n\
ANTHROPIC_API_KEY={anthropic_api_key}\n\
XAI_API_KEY={xai_api_key}\n\
DEEPSEEK_API_KEY={deepseek_api_key}\n\
GLM_API_KEY={zai_api_key}\n\
MINIMAX_API_KEY={minimax_api_key}\n\
KIMI_API_KEY={moonshot_api_key}\n\
QWEN_API_KEY={qwen_api_key}\n"
        ),
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
        content.contains(&fake_secrets::removed_rule(&google_api_key)),
        "draft should include gemini/google api key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&anthropic_api_key)),
        "draft should include anthropic key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&xai_api_key)),
        "draft should include xai key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&deepseek_api_key)),
        "draft should include deepseek key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&zai_api_key)),
        "draft should include glm(z.ai) key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&minimax_api_key)),
        "draft should include minimax key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&moonshot_api_key)),
        "draft should include kimi key: {}",
        content
    );
    assert!(
        content.contains(&fake_secrets::removed_rule(&qwen_api_key)),
        "draft should include qwen key: {}",
        content
    );
}
