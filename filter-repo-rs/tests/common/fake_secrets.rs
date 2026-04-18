#![allow(dead_code)]
#![allow(unused_imports)]

fn concat(parts: &[&str]) -> String {
    parts.concat()
}

fn join(parts: &[&str], separator: &str) -> String {
    parts.join(separator)
}

pub fn replace_rule(secret: &str, replacement: &str) -> String {
    format!("{secret}==>{replacement}")
}

pub fn replace_rule_line(secret: &str, replacement: &str) -> String {
    format!("{}\n", replace_rule(secret, replacement))
}

pub fn removed_rule(secret: &str) -> String {
    replace_rule(secret, "***REMOVED***")
}

pub fn aws_access_key_id() -> String {
    concat(&["AKIA", "1234567890ABCDEF"])
}

pub fn aws_secret_access_key() -> String {
    concat(&["abcdEFGHijklMNOP", "qrstUVWXyz0123456789+/AB"])
}

pub fn google_api_key() -> String {
    concat(&["AIza", "12345678901234567890123456789012345"])
}

pub fn openai_api_key() -> String {
    concat(&["sk-", "abcdefghijklmnopqrstuvwxyz1234567890ABC"])
}

pub fn openai_project_key() -> String {
    concat(&["sk-proj-", "abcdefghijklmnopqrstuvwxyz1234567890ab"])
}

pub fn gitlab_token() -> String {
    concat(&["glpat-", "abcDEF0123456789uvwxyzABCD"])
}

pub fn npm_token() -> String {
    concat(&["npm_", "1234567890abcdefghijklmnopqrstuvwxyz"])
}

pub fn anthropic_api_key() -> String {
    concat(&["sk-ant-api03-", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab"])
}

pub fn xai_api_key() -> String {
    concat(&["xai-", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab"])
}

pub fn deepseek_api_key() -> String {
    concat(&["deepseek_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"])
}

pub fn zai_api_key() -> String {
    concat(&["zai-", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789ab"])
}

pub fn minimax_api_key() -> String {
    concat(&["minimax_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"])
}

pub fn moonshot_api_key() -> String {
    concat(&["moonshot_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"])
}

pub fn qwen_api_key() -> String {
    concat(&["qwen_", "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"])
}

pub fn github_pat() -> String {
    concat(&["ghp_", "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh"])
}

pub fn slack_token() -> String {
    concat(&[
        "xoxb",
        "-123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx",
    ])
}

pub fn slack_webhook_url() -> String {
    let domain = join(&["hooks", "slack", "com"], ".");
    format!("https://{domain}/services/T12345678/B12345678/abcdefghijklmnopqrstuvwx")
}

pub fn stripe_live_secret() -> String {
    join(&["sk", "live", "abcdefghijklmnopqrstuvwxyz123456"], "_")
}

pub fn super_secret_123() -> String {
    concat(&["super", "Secret", "123"])
}

pub fn secret_123() -> String {
    concat(&["secret", "123"])
}

pub fn custom_secret_2026() -> String {
    join(&["ZZZ", "CUSTOM", "SECRET", "2026"], "-")
}

pub fn secret_abc_123() -> String {
    join(&["SECRET", "ABC", "123"], "-")
}

pub fn secret_inline_123() -> String {
    join(&["SECRET", "INLINE", "123"], "-")
}

pub fn secret_numbered(n: usize) -> String {
    join(&["SECRET", &n.to_string()], "-")
}

pub fn secret_token_value() -> String {
    join(&["SECRET", "TOKEN", "VALUE"], "_")
}

pub fn secret_value_alpha() -> String {
    join(&["SECRET", "VALUE", "ALPHA"], "_")
}

pub fn private_token_beta() -> String {
    join(&["PRIVATE", "TOKEN", "BETA"], "_")
}

pub fn internal_key_gamma() -> String {
    join(&["INTERNAL", "KEY", "GAMMA"], "_")
}

pub fn api_key_abc() -> String {
    join(&["api", "key", "abc"], "_")
}

pub fn password123() -> String {
    concat(&["password", "123"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_helpers_build_expected_values() {
        assert_eq!(aws_access_key_id(), ["AKIA", "1234567890ABCDEF"].concat());
        assert_eq!(
            openai_api_key(),
            ["sk-", "abcdefghijklmnopqrstuvwxyz1234567890ABC"].concat()
        );
        assert_eq!(
            google_api_key(),
            ["AIza", "12345678901234567890123456789012345"].concat()
        );
        assert_eq!(
            slack_webhook_url(),
            format!(
                "https://{}/services/T12345678/B12345678/abcdefghijklmnopqrstuvwx",
                ["hooks", "slack", "com"].join(".")
            )
        );
    }

    #[test]
    fn generic_helpers_build_expected_values() {
        assert_eq!(secret_abc_123(), ["SECRET", "ABC", "123"].join("-"));
        assert_eq!(secret_inline_123(), ["SECRET", "INLINE", "123"].join("-"));
        assert_eq!(secret_numbered(3), ["SECRET", "3"].join("-"));
        assert_eq!(secret_token_value(), ["SECRET", "TOKEN", "VALUE"].join("_"));
    }
}
