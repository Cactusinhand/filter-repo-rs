use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use filter_repo_rs::detect::{collect_blob_detections, SecretPattern};
use regex::bytes::Regex;

fn build_default_patterns() -> Vec<SecretPattern> {
    vec![
        SecretPattern {
            name: "aws_access_key_id".into(),
            regex: Regex::new(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b").unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "github_token".into(),
            regex: Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36}\b").unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "slack_token".into(),
            regex: Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{10,128}\b").unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "google_api_key".into(),
            regex: Regex::new(r"\bAIza[0-9A-Za-z_-]{35}\b").unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "jwt".into(),
            regex: Regex::new(
                r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9._-]{10,}\.[A-Za-z0-9._-]{10,}\b",
            )
            .unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "openai_api_key".into(),
            regex: Regex::new(r"\b(?:sk-|sk-proj-)[A-Za-z0-9_-]{20,200}\b").unwrap(),
            capture_group: None,
        },
        SecretPattern {
            name: "assignment_value".into(),
            regex: Regex::new(
                r#"(?i)\b(?:api[_-]?key|token|secret|password|passwd)\b\s*[:=]\s*["']?([A-Za-z0-9_./+=:@-]{8,256})["']?"#,
            )
            .unwrap(),
            capture_group: Some(1),
        },
        SecretPattern {
            name: "db_url_password".into(),
            regex: Regex::new(r"\b[a-z][a-z0-9+.-]*://[^/\s:@]+:([^/\s@]{8,})@[^/\s]+").unwrap(),
            capture_group: Some(1),
        },
    ]
}

/// Generate a blob payload of `size` bytes. When `inject_secrets` > 0, scatter
/// that many fake secrets throughout the content.
fn make_blob(size: usize, inject_secrets: usize) -> Vec<u8> {
    let filler =
        b"const foo = 42;\nlet bar = \"hello world\";\nfunction process(data) { return data; }\n";
    // Construct fake secrets at runtime via concat so the full literal patterns
    // don't appear in this source file — mirrors the approach in tests/detect_secrets.rs.
    let secrets: Vec<Vec<u8>> = vec![
        [b"AKIA" as &[u8], b"IOSFODNN7EXAMPLE1"].concat(),
        [b"ghp_" as &[u8], b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh"].concat(),
        [
            b"xoxb" as &[u8],
            b"-123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx",
        ]
        .concat(),
        [
            b"sk-proj-" as &[u8],
            b"abcdefghijklmnopqrstuvwxyz1234567890ab",
        ]
        .concat(),
    ];

    let mut buf = Vec::with_capacity(size);
    let mut secret_idx = 0;
    let interval = if inject_secrets > 0 {
        size / (inject_secrets + 1)
    } else {
        usize::MAX
    };

    while buf.len() < size {
        if inject_secrets > 0
            && secret_idx < inject_secrets
            && buf.len() >= interval * (secret_idx + 1)
        {
            let s = &secrets[secret_idx % secrets.len()];
            let remaining = size - buf.len();
            let take = s.len().min(remaining);
            buf.extend_from_slice(&s[..take]);
            buf.push(b'\n');
            secret_idx += 1;
        } else {
            let remaining = size - buf.len();
            let take = filler.len().min(remaining);
            buf.extend_from_slice(&filler[..take]);
        }
    }
    buf.truncate(size);
    buf
}

// ---------------------------------------------------------------------------
// Single-blob detection
// ---------------------------------------------------------------------------

fn bench_detect_single_blob(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_single_blob");
    let patterns = build_default_patterns();
    let oid = "abcdef1234567890abcdef1234567890abcdef12";

    let sizes: &[(usize, &str)] = &[
        (1024, "1KB"),
        (64 * 1024, "64KB"),
        (512 * 1024, "512KB"),
        (2 * 1024 * 1024, "2MB"),
    ];

    for &(size, label) in sizes {
        // No secrets (miss path — should be fast)
        let clean_blob = make_blob(size, 0);
        group.bench_with_input(BenchmarkId::new("clean", label), &clean_blob, |b, blob| {
            b.iter(|| {
                collect_blob_detections(
                    black_box(blob),
                    black_box(oid),
                    black_box(Some("src/main.rs")),
                    black_box(&patterns),
                )
            })
        });

        // With secrets (hit path)
        let dirty_blob = make_blob(size, 4);
        group.bench_with_input(
            BenchmarkId::new("with_secrets", label),
            &dirty_blob,
            |b, blob| {
                b.iter(|| {
                    collect_blob_detections(
                        black_box(blob),
                        black_box(oid),
                        black_box(Some("src/config.rs")),
                        black_box(&patterns),
                    )
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Pattern count scaling
// ---------------------------------------------------------------------------

fn bench_detect_pattern_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("detect_pattern_scaling");
    let oid = "abcdef1234567890abcdef1234567890abcdef12";
    let blob = make_blob(64 * 1024, 0); // 64KB clean blob

    // Test with increasing pattern counts
    let all_patterns = build_default_patterns();
    for &n in &[2usize, 4, 8] {
        let patterns = &all_patterns[..n.min(all_patterns.len())];
        group.bench_with_input(BenchmarkId::new("patterns", n), &n, |b, _| {
            b.iter(|| {
                collect_blob_detections(
                    black_box(&blob),
                    black_box(oid),
                    black_box(Some("src/lib.rs")),
                    black_box(patterns),
                )
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_detect_single_blob,
    bench_detect_pattern_scaling
);
criterion_main!(benches);
