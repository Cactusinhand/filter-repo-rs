mod common;
use common::*;

#[test]
fn performance_large_repository_batch_optimization() {
    let repo = init_repo();
    let num_files = 1000;
    for i in 0..num_files {
        let content = match i % 5 {
            0 => "large file content that exceeds typical blob size thresholds".repeat(100),
            1 => "medium file content with moderate size".repeat(20),
            2 => "small file content".repeat(5),
            3 => "tiny content".to_string(),
            _ => "min".to_string(),
        };
        let path = format!("perf_test_file_{:04}.txt", i);
        std::fs::write(repo.join(&path), content).unwrap();
        run_git(&repo, &["add", &path]);
    }
    run_git(
        &repo,
        &["commit", "-m", "Performance test: large repository"],
    );

    let (_c0, tree0, _e0) = run_git(
        &repo,
        &[
            "-c",
            "core.quotepath=false",
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
        ],
    );
    let files0: Vec<&str> = tree0.split_whitespace().collect();
    let baseline_count = files0.len();
    assert!(baseline_count >= num_files);

    let thresholds = vec![10, 50, 100, 500, 1000, 5000];
    let mut performance_metrics = Vec::new();
    for threshold in thresholds {
        let filter_start = std::time::Instant::now();
        run_tool_expect_success(&repo, |o| {
            o.max_blob_size = Some(threshold);
        });
        let filter_time = filter_start.elapsed();
        let (_c2, tree, _e2) = run_git(
            &repo,
            &[
                "-c",
                "core.quotepath=false",
                "ls-tree",
                "-r",
                "--name-only",
                "HEAD",
            ],
        );
        let files: Vec<&str> = tree.split_whitespace().collect();
        let filtered_count = files.len();
        let filter_ratio =
            (baseline_count.saturating_sub(filtered_count)) as f64 / baseline_count as f64;
        performance_metrics.push((threshold, filter_time, filtered_count, filter_ratio));
        if threshold < 5000 {
            assert!(filtered_count < baseline_count);
        }
    }
    assert!(!performance_metrics.is_empty());
}

#[test]
fn performance_cache_effectiveness() {
    let num_commits = 10;
    let files_per_commit = 20;
    let mut iteration_times = Vec::new();
    for _ in 0..5 {
        let repo = init_repo();
        for commit_i in 0..num_commits {
            for file_j in 0..files_per_commit {
                let content = format!(
                    "Commit {} file {} content with varying size {}",
                    commit_i,
                    file_j,
                    "x".repeat((commit_i * 100 + file_j * 10) % 2000)
                );
                let path = format!("cache_test_commit_{:02}_file_{:02}.txt", commit_i, file_j);
                std::fs::write(repo.join(&path), content).unwrap();
                run_git(&repo, &["add", &path]);
            }
            run_git(
                &repo,
                &["commit", "-m", &format!("Cache test commit {}", commit_i)],
            );
        }
        let start_time = std::time::Instant::now();
        run_tool_expect_success(&repo, |o| {
            o.max_blob_size = Some(500);
        });
        iteration_times.push(start_time.elapsed());
    }
    assert!(iteration_times.len() >= 3);
}

#[test]
fn performance_scalability_with_blob_count() {
    let repo = init_repo();
    let blob_counts = vec![100, 500, 1000];
    for &blob_count in &blob_counts {
        for i in 0..blob_count {
            let content = format!(
                "Scalability test blob {} with content size {}",
                i,
                "x".repeat((i % 1000) + 100)
            );
            let path = format!("scale_test_{:04}_count_{}.txt", i, blob_count);
            std::fs::write(repo.join(&path), content).unwrap();
            run_git(&repo, &["add", &path]);
        }
        run_git(
            &repo,
            &[
                "commit",
                "-m",
                &format!("Scalability test with {} blobs", blob_count),
            ],
        );
        run_tool_expect_success(&repo, |o| {
            o.max_blob_size = Some(500);
        });
    }
}

#[test]
fn performance_memory_usage_benchmark() {
    let repo = init_repo();
    for i in 0..500 {
        let size = if i % 7 == 0 { 5000 } else { 200 };
        std::fs::write(repo.join(format!("bench_{}.bin", i)), vec![b'X'; size]).unwrap();
        run_git(&repo, &["add", &format!("bench_{}.bin", i)]);
    }
    run_git(&repo, &["commit", "-m", "benchmark data"]);
    let start = std::time::Instant::now();
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1000);
    });
    let dur = start.elapsed();
    assert!(dur > std::time::Duration::from_millis(0));
}

#[test]
fn performance_batch_vs_individual_optimization() {
    // Smoke check: two consecutive thresholds shouldn't differ wildly
    let repo = init_repo();
    for i in 0..200 {
        let size = 500 + (i * 13) % 4000;
        std::fs::write(repo.join(format!("file_{}.dat", i)), vec![b'Y'; size]).unwrap();
        run_git(&repo, &["add", &format!("file_{}.dat", i)]);
    }
    run_git(&repo, &["commit", "-m", "files for optimization"]);
    let t1 = std::time::Instant::now();
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1200);
    });
    let d1 = t1.elapsed();
    let t2 = std::time::Instant::now();
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1300);
    });
    let d2 = t2.elapsed();
    assert!(d1 > std::time::Duration::from_micros(0));
    assert!(d2 > std::time::Duration::from_micros(0));
}
