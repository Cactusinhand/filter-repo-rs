use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::io::BufReader;

use filter_repo_rs::commit::MailmapRewriter;

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

fn bench_mailmap_rewrite(c: &mut Criterion) {
    let mut group = c.benchmark_group("MailmapRewriter");

    let rule_counts: &[usize] = &[5, 50, 200];

    for &n in rule_counts {
        let rewriter = make_mailmap(n);

        // Match: rewrite an email that's in the map (last entry — worst case linear scan)
        let match_line = format!(
            "author Some Author <old{}@example.com> 1700000000 +0000",
            n - 1
        );
        group.bench_with_input(
            BenchmarkId::new("rewrite_line/match", n),
            match_line.as_bytes(),
            |b, line| {
                b.iter(|| rewriter.rewrite_line(black_box(line)));
            },
        );

        // Miss: email not in the map
        let miss_line = b"author Unknown <nobody@nowhere.org> 1700000000 +0000";
        group.bench_with_input(
            BenchmarkId::new("rewrite_line/miss", n),
            miss_line.as_slice(),
            |b, line| {
                b.iter(|| rewriter.rewrite_line(black_box(line)));
            },
        );

        // Committer line (different prefix)
        let committer_line = format!("committer Committer <old0@example.com> 1700000000 +0000");
        group.bench_with_input(
            BenchmarkId::new("rewrite_line/committer", n),
            committer_line.as_bytes(),
            |b, line| {
                b.iter(|| rewriter.rewrite_line(black_box(line)));
            },
        );

        // Non-identity line (should bail out fast)
        let other_line = b"data 12345\n";
        group.bench_with_input(
            BenchmarkId::new("rewrite_line/non_identity", n),
            other_line.as_slice(),
            |b, line| {
                b.iter(|| rewriter.rewrite_line(black_box(line)));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_mailmap_rewrite);
criterion_main!(benches);
