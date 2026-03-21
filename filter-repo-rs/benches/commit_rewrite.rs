use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::io::BufReader;

use filter_repo_rs::commit::{AuthorRewriter, MailmapRewriter};

// ---------------------------------------------------------------------------
// AuthorRewriter (AhoCorasick-based email/name rewriting)
// ---------------------------------------------------------------------------

fn make_author_rewriter(n: usize) -> AuthorRewriter {
    let mut content = String::new();
    for i in 0..n {
        content.push_str(&format!(
            "old_author_{}@corp.com==>new_author_{}@newcorp.com\n",
            i, i
        ));
    }
    let reader = BufReader::new(content.as_bytes());
    AuthorRewriter::from_reader(reader).unwrap()
}

fn bench_author_rewriter(c: &mut Criterion) {
    let mut group = c.benchmark_group("AuthorRewriter");

    let rule_counts: &[usize] = &[5, 50, 200, 1000];

    for &n in rule_counts {
        let rewriter = make_author_rewriter(n);

        // Hit: last entry (worst-case for linear scan, but AhoCorasick should be O(len))
        let hit_line = format!(
            "author Some Author <old_author_{}@corp.com> 1700000000 +0000",
            n - 1
        );
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| rewriter.rewrite(black_box(hit_line.as_bytes())))
        });

        // Miss: email not in the map
        let miss_line = "author Unknown <unknown@nowhere.com> 1700000000 +0000";
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| rewriter.rewrite(black_box(miss_line.as_bytes())))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// MailmapRewriter at larger scales (complement to mailmap.rs bench)
// ---------------------------------------------------------------------------

fn make_mailmap(n: usize) -> MailmapRewriter {
    let mut content = String::new();
    for i in 0..n {
        content.push_str(&format!(
            "New Name{0} <new{0}@example.com> <old{0}@example.com>\n",
            i
        ));
    }
    let reader = BufReader::new(content.as_bytes());
    MailmapRewriter::from_reader(reader).unwrap()
}

fn bench_mailmap_large_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("MailmapRewriter_large");

    // Test scaling beyond the 200-rule limit in mailmap.rs
    for &n in &[500usize, 1000, 5000] {
        let rewriter = make_mailmap(n);

        // Hit last entry
        let hit_line = format!(
            "author Some Author <old{}@example.com> 1700000000 +0000",
            n - 1
        );
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| rewriter.rewrite_line(black_box(hit_line.as_bytes())))
        });

        // Miss
        let miss_line = "author Unknown <nobody@nowhere.com> 1700000000 +0000";
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| rewriter.rewrite_line(black_box(miss_line.as_bytes())))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Batch: simulate rewriting N commit lines
// ---------------------------------------------------------------------------

fn bench_commit_line_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("commit_line_batch");

    let author_rewriter = make_author_rewriter(50);
    let mailmap_rewriter = make_mailmap(50);

    // Generate realistic commit author/committer lines
    let lines: Vec<String> = (0..500)
        .map(|i| {
            if i % 3 == 0 {
                // Hit author rewriter
                format!(
                    "author Dev{0} <old_author_{0}@corp.com> {1} +0000",
                    i % 50,
                    1700000000 + i * 100
                )
            } else if i % 3 == 1 {
                // Hit mailmap
                format!(
                    "committer Dev{0} <old{0}@example.com> {1} +0000",
                    i % 50,
                    1700000000 + i * 100
                )
            } else {
                // Miss both
                format!(
                    "author External <ext_{}@other.com> {} +0000",
                    i,
                    1700000000 + i * 100
                )
            }
        })
        .collect();

    group.bench_function("500_author_rewrites", |b| {
        b.iter(|| {
            for line in &lines {
                black_box(author_rewriter.rewrite(line.as_bytes()));
            }
        })
    });

    group.bench_function("500_mailmap_rewrites", |b| {
        b.iter(|| {
            for line in &lines {
                black_box(mailmap_rewriter.rewrite_line(line.as_bytes()));
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_author_rewriter,
    bench_mailmap_large_scale,
    bench_commit_line_batch
);
criterion_main!(benches);
