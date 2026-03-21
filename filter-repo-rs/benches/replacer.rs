use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::io::Write;
use tempfile::NamedTempFile;

use filter_repo_rs::message::blob_regex::RegexReplacer as BlobRegexReplacer;
use filter_repo_rs::message::MessageReplacer;

fn make_rules_file(rules: &[u8]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(rules).unwrap();
    f.flush().unwrap();
    f
}

fn make_payload(size: usize, hit: bool) -> Vec<u8> {
    // Build a payload of the requested size.
    // When `hit` is true, scatter replaceable tokens throughout;
    // when false, use content that won't match any rule.
    if hit {
        let token = b"SECRET_TOKEN_VALUE";
        let filler = b"the quick brown fox jumps over the lazy dog ";
        let mut buf = Vec::with_capacity(size);
        let mut i = 0;
        while buf.len() < size {
            if i % 5 == 0 && buf.len() + token.len() <= size {
                buf.extend_from_slice(token);
            } else if buf.len() + filler.len() <= size {
                buf.extend_from_slice(filler);
            } else {
                buf.push(b'x');
            }
            i += 1;
        }
        buf.truncate(size);
        buf
    } else {
        vec![b'x'; size]
    }
}

fn bench_message_replacer(c: &mut Criterion) {
    let rules = b"SECRET_TOKEN_VALUE==>REDACTED\npassword123==>***\napi_key_abc==>***\n";
    let rules_file = make_rules_file(rules);
    let replacer = MessageReplacer::from_file(rules_file.path()).unwrap();

    let sizes: &[usize] = &[1_024, 64 * 1_024, 1_024 * 1_024];

    let mut group = c.benchmark_group("MessageReplacer");

    for &size in sizes {
        let label = if size >= 1_024 * 1_024 {
            format!("{}MB", size / (1_024 * 1_024))
        } else {
            format!("{}KB", size / 1_024)
        };

        // No-match path (the common case we optimized)
        let payload_miss = make_payload(size, false);
        group.bench_with_input(
            BenchmarkId::new("apply_with_change/miss", &label),
            &payload_miss,
            |b, data| {
                b.iter(|| replacer.apply_with_change(black_box(data.clone())));
            },
        );

        // Match path
        let payload_hit = make_payload(size, true);
        group.bench_with_input(
            BenchmarkId::new("apply_with_change/hit", &label),
            &payload_hit,
            |b, data| {
                b.iter(|| replacer.apply_with_change(black_box(data.clone())));
            },
        );

        // would_change alone (to see the scan cost)
        group.bench_with_input(
            BenchmarkId::new("would_change/miss", &label),
            &payload_miss,
            |b, data| {
                b.iter(|| replacer.would_change(black_box(data)));
            },
        );
    }

    group.finish();
}

fn bench_blob_regex_replacer(c: &mut Criterion) {
    let rules = b"regex:[A-Z]{5,}_[A-Z]+_[A-Z]+==>REDACTED\nregex:\\b\\d{3}-\\d{2}-\\d{4}\\b==>SSN_REDACTED\n";
    let rules_file = make_rules_file(rules);
    let replacer = BlobRegexReplacer::from_file(rules_file.path())
        .unwrap()
        .unwrap();

    let sizes: &[usize] = &[1_024, 64 * 1_024, 1_024 * 1_024];

    let mut group = c.benchmark_group("BlobRegexReplacer");

    for &size in sizes {
        let label = if size >= 1_024 * 1_024 {
            format!("{}MB", size / (1_024 * 1_024))
        } else {
            format!("{}KB", size / 1_024)
        };

        let payload_miss = vec![b'x'; size];
        group.bench_with_input(
            BenchmarkId::new("apply_regex_with_change/miss", &label),
            &payload_miss,
            |b, data| {
                b.iter(|| replacer.apply_regex_with_change(black_box(data.clone())));
            },
        );

        let payload_hit = make_payload(size, true);
        group.bench_with_input(
            BenchmarkId::new("apply_regex_with_change/hit", &label),
            &payload_hit,
            |b, data| {
                b.iter(|| replacer.apply_regex_with_change(black_box(data.clone())));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_message_replacer, bench_blob_regex_replacer);
criterion_main!(benches);
