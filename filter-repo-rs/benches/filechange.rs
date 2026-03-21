use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use filter_repo_rs::filechange::handle_file_change_line;
use filter_repo_rs::opts::Options;

fn make_opts_no_filter() -> Options {
    Options::default()
}

fn make_opts_with_paths(paths: Vec<Vec<u8>>) -> Options {
    let mut opts = Options::default();
    opts.paths = paths;
    opts
}

fn make_opts_with_globs(globs: Vec<Vec<u8>>) -> Options {
    let mut opts = Options::default();
    opts.path_globs = globs;
    opts
}

fn make_opts_with_renames(renames: Vec<(Vec<u8>, Vec<u8>)>) -> Options {
    let mut opts = Options::default();
    opts.paths = vec![b"src/".to_vec()]; // keep src/
    opts.path_renames = renames;
    opts
}

// ---------------------------------------------------------------------------
// Parsing speed
// ---------------------------------------------------------------------------

fn bench_parse_filechange(c: &mut Criterion) {
    let mut group = c.benchmark_group("filechange_parse");
    let opts = make_opts_no_filter();

    // M (modify) line — most common in fast-export streams
    let modify_line = b"M 100644 :42 src/main.rs\n";
    group.bench_function("modify_simple", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts))
    });

    // M with quoted path (C-style)
    let modify_quoted = b"M 100644 :42 \"src/my module/file name.rs\"\n";
    group.bench_function("modify_quoted", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_quoted), &opts))
    });

    // D (delete) line
    let delete_line = b"D old/removed/file.txt\n";
    group.bench_function("delete", |b| {
        b.iter(|| handle_file_change_line(black_box(delete_line), &opts))
    });

    // R (rename) line — two paths
    let rename_line = b"R old/path/file.rs new/path/file.rs\n";
    group.bench_function("rename", |b| {
        b.iter(|| handle_file_change_line(black_box(rename_line), &opts))
    });

    // C (copy) line
    let copy_line = b"C src/original.rs src/copy.rs\n";
    group.bench_function("copy", |b| {
        b.iter(|| handle_file_change_line(black_box(copy_line), &opts))
    });

    // deleteall
    let deleteall_line = b"deleteall\n";
    group.bench_function("deleteall", |b| {
        b.iter(|| handle_file_change_line(black_box(deleteall_line), &opts))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Path filtering (the hot inner loop)
// ---------------------------------------------------------------------------

fn bench_path_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("filechange_filter");

    let modify_line = b"M 100644 :42 src/components/auth/login.rs\n";

    // No filter (passthrough)
    let opts_none = make_opts_no_filter();
    group.bench_function("no_filter", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_none))
    });

    // Prefix filter — hit
    let opts_prefix_hit = make_opts_with_paths(vec![b"src/".to_vec()]);
    group.bench_function("prefix/hit", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_prefix_hit))
    });

    // Prefix filter — miss
    let opts_prefix_miss = make_opts_with_paths(vec![b"lib/".to_vec()]);
    group.bench_function("prefix/miss", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_prefix_miss))
    });

    // Glob filter — hit
    let opts_glob_hit = make_opts_with_globs(vec![b"**/*.rs".to_vec()]);
    group.bench_function("glob/hit", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_glob_hit))
    });

    // Glob filter — miss
    let opts_glob_miss = make_opts_with_globs(vec![b"**/*.py".to_vec()]);
    group.bench_function("glob/miss", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_glob_miss))
    });

    // Multiple prefix filters (5, 20)
    for &n in &[5usize, 20] {
        let paths: Vec<Vec<u8>> = (0..n)
            .map(|i| format!("module_{}/", i).into_bytes())
            .collect();
        let opts = make_opts_with_paths(paths);
        group.bench_with_input(BenchmarkId::new("prefix_miss_n", n), &n, |b, _| {
            b.iter(|| handle_file_change_line(black_box(modify_line), &opts))
        });
    }

    // Multiple glob filters (5, 20)
    for &n in &[5usize, 20] {
        let globs: Vec<Vec<u8>> = (0..n)
            .map(|i| format!("module_{}/**/*.rs", i).into_bytes())
            .collect();
        let opts = make_opts_with_globs(globs);
        group.bench_with_input(BenchmarkId::new("glob_miss_n", n), &n, |b, _| {
            b.iter(|| handle_file_change_line(black_box(modify_line), &opts))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Path rename + re-encode
// ---------------------------------------------------------------------------

fn bench_path_rename(c: &mut Criterion) {
    let mut group = c.benchmark_group("filechange_rename");

    let modify_line = b"M 100644 :42 src/old_module/file.rs\n";

    // Single rename rule
    let opts_single = make_opts_with_renames(vec![(
        b"src/old_module/".to_vec(),
        b"src/new_module/".to_vec(),
    )]);
    group.bench_function("single_rename", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_single))
    });

    // Multiple rename rules (10) — only first matches
    let mut renames: Vec<(Vec<u8>, Vec<u8>)> = (0..10)
        .map(|i| {
            (
                format!("pkg_{}/", i).into_bytes(),
                format!("package_{}/", i).into_bytes(),
            )
        })
        .collect();
    renames.push((b"src/old_module/".to_vec(), b"src/new_module/".to_vec()));
    let mut opts_multi = make_opts_with_renames(renames);
    opts_multi.paths = vec![b"src/".to_vec(), b"pkg_".to_vec()];
    group.bench_function("multi_rename_10", |b| {
        b.iter(|| handle_file_change_line(black_box(modify_line), &opts_multi))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Batch: simulate processing a fast-export stream of N filechanges
// ---------------------------------------------------------------------------

fn bench_batch_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("filechange_batch");

    let opts = make_opts_with_globs(vec![b"src/**/*.rs".to_vec()]);

    let lines: Vec<Vec<u8>> = (0..1000)
        .map(|i| {
            if i % 10 == 0 {
                format!("D old/removed_{}.txt\n", i).into_bytes()
            } else {
                format!("M 100644 :{} src/mod_{}/file_{}.rs\n", i, i / 50, i).into_bytes()
            }
        })
        .collect();

    group.bench_function("1000_lines", |b| {
        b.iter(|| {
            let mut kept = 0usize;
            for line in &lines {
                let outcome = handle_file_change_line(black_box(line), &opts).unwrap();
                if outcome.line.is_some() {
                    kept += 1;
                }
            }
            kept
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_filechange,
    bench_path_filtering,
    bench_path_rename,
    bench_batch_processing
);
criterion_main!(benches);
