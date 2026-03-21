use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use filter_repo_rs::pathutil::{
    dequote_c_style_bytes, encode_path_for_fi, enquote_c_style_bytes, glob_match_bytes,
};

// ---------------------------------------------------------------------------
// glob_match_bytes
// ---------------------------------------------------------------------------

fn bench_glob_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("glob_match_bytes");

    // Realistic path depths
    let shallow_path = b"src/main.rs";
    let medium_path = b"src/components/auth/login/LoginForm.tsx";
    let deep_path = b"packages/core/src/internal/utils/parsing/json/helpers.rs";

    // Pattern: exact prefix
    group.bench_function("prefix/shallow", |b| {
        b.iter(|| glob_match_bytes(black_box(b"src/*"), black_box(shallow_path)))
    });

    // Pattern: single wildcard miss
    group.bench_function("single_star/miss", |b| {
        b.iter(|| glob_match_bytes(black_box(b"lib/*"), black_box(medium_path)))
    });

    // Pattern: ** (double star) — the expensive recursive case
    group.bench_function("double_star/shallow", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.rs"), black_box(shallow_path)))
    });
    group.bench_function("double_star/medium", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.tsx"), black_box(medium_path)))
    });
    group.bench_function("double_star/deep", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.rs"), black_box(deep_path)))
    });
    group.bench_function("double_star/miss_deep", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.py"), black_box(deep_path)))
    });

    // Pattern: prefix + double star
    group.bench_function("prefix_double_star/hit", |b| {
        b.iter(|| glob_match_bytes(black_box(b"packages/**/helpers.rs"), black_box(deep_path)))
    });
    group.bench_function("prefix_double_star/miss", |b| {
        b.iter(|| glob_match_bytes(black_box(b"vendor/**/helpers.rs"), black_box(deep_path)))
    });

    // Pattern: multiple wildcards
    group.bench_function("multi_wildcard", |b| {
        b.iter(|| {
            glob_match_bytes(
                black_box(b"src/*/auth/*/Login*.tsx"),
                black_box(medium_path),
            )
        })
    });

    // Worst-case: many ** on a long path
    let very_deep = b"a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/file.txt";
    group.bench_function("double_star/very_deep_hit", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.txt"), black_box(very_deep.as_ref())))
    });
    group.bench_function("double_star/very_deep_miss", |b| {
        b.iter(|| glob_match_bytes(black_box(b"**/*.rs"), black_box(very_deep.as_ref())))
    });

    // Batch: simulate filtering N paths against a single glob
    let paths: Vec<Vec<u8>> = (0..1000)
        .map(|i| format!("src/mod_{}/lib.rs", i).into_bytes())
        .collect();
    group.bench_function("batch_1000_paths", |b| {
        b.iter(|| {
            let pat = b"src/*/lib.rs";
            paths.iter().filter(|p| glob_match_bytes(pat, p)).count()
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Multi-glob filter (simulates path_matches with N globs)
// ---------------------------------------------------------------------------

fn bench_multi_glob_filter(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_glob_filter");

    let globs: Vec<Vec<u8>> = vec![
        b"**/*.rs".to_vec(),
        b"**/*.toml".to_vec(),
        b"src/**".to_vec(),
        b"tests/**/*.rs".to_vec(),
        b"benches/**".to_vec(),
    ];
    let path = b"src/components/auth/login.rs";

    for &n in &[1, 3, 5] {
        let active_globs = &globs[..n];
        group.bench_with_input(BenchmarkId::new("globs", n), &n, |b, _| {
            b.iter(|| {
                active_globs
                    .iter()
                    .any(|g| glob_match_bytes(g, black_box(path)))
            })
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Path encoding / decoding
// ---------------------------------------------------------------------------

fn bench_path_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_encoding");

    // Simple ASCII path (no quoting needed)
    let simple = b"src/main.rs";
    group.bench_function("encode_fi/simple", |b| {
        b.iter(|| encode_path_for_fi(black_box(simple)))
    });

    // Path with spaces (needs C-style quoting)
    let with_spaces = b"src/my module/file name.rs";
    group.bench_function("encode_fi/spaces", |b| {
        b.iter(|| encode_path_for_fi(black_box(with_spaces)))
    });

    // Path with non-ASCII (needs octal escaping)
    let non_ascii = "src/données/résumé.txt".as_bytes();
    group.bench_function("encode_fi/non_ascii", |b| {
        b.iter(|| encode_path_for_fi(black_box(non_ascii)))
    });

    // Round-trip: enquote then dequote
    let quoted = enquote_c_style_bytes(non_ascii);
    group.bench_function("dequote/non_ascii", |b| {
        b.iter(|| dequote_c_style_bytes(black_box(&quoted)))
    });

    // Batch: encode 500 paths
    let paths: Vec<Vec<u8>> = (0..500)
        .map(|i| format!("src/module_{}/component_{}.rs", i / 10, i).into_bytes())
        .collect();
    group.bench_function("encode_fi/batch_500", |b| {
        b.iter(|| {
            for p in &paths {
                black_box(encode_path_for_fi(p));
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_glob_match,
    bench_multi_glob_filter,
    bench_path_encoding
);
criterion_main!(benches);
