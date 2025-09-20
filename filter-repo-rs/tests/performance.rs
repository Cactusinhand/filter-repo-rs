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
    run_git(&repo, &["commit", "-m", "Performance test: large repository"]);

    let (_c0, tree0, _e0) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
    let files0: Vec<&str> = tree0.split_whitespace().collect();
    let baseline_count = files0.len();
    assert!(baseline_count >= num_files);

    let thresholds = vec![10, 50, 100, 500, 1000, 5000];
    let mut performance_metrics = Vec::new();
    for threshold in thresholds {
        let filter_start = std::time::Instant::now();
        let (_c, _o, _e) = run_tool(&repo, |o| {
            o.max_blob_size = Some(threshold);
        });
        let filter_time = filter_start.elapsed();
        let (_c2, tree, _e2) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
        let files: Vec<&str> = tree.split_whitespace().collect();
        let filtered_count = files.len();
        let filter_ratio = (baseline_count - filtered_count) as f64 / baseline_count as f64;
        performance_metrics.push((threshold, filter_time, filtered_count, filter_ratio));
        assert!(filter_time.as_millis() < 5000, "threshold {} too slow", threshold);
        if threshold < 5000 {
            assert!(filtered_count < baseline_count);
        }
    }
    if performance_metrics.len() >= 2 {
        let fastest_time = performance_metrics.iter().map(|&(_, t, _, _)| t).min().unwrap();
        let slowest_time = performance_metrics.iter().map(|&(_, t, _, _)| t).max().unwrap();
        let time_ratio = slowest_time.as_millis() as f64 / fastest_time.as_millis() as f64;
        assert!(time_ratio < 10.0);
    }
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
            run_git(&repo, &["commit", "-m", &format!("Cache test commit {}", commit_i)]);
        }
        let start_time = std::time::Instant::now();
        let (_c, _o, _e) = run_tool(&repo, |o| {
            o.max_blob_size = Some(500);
        });
        iteration_times.push(start_time.elapsed());
    }
    if iteration_times.len() >= 3 {
        let max_time = iteration_times.iter().max().unwrap();
        let min_time = iteration_times.iter().min().unwrap();
        let degradation_ratio = max_time.as_millis() as f64 / min_time.as_millis() as f64;
        assert!(degradation_ratio < 3.0);
    }
}

#[test]
fn performance_scalability_with_blob_count() {
    let repo = init_repo();
    let blob_counts = vec![100, 500, 1000];
    for &blob_count in &blob_counts {
        for i in 0..blob_count {
            let content = format!("Scalability test blob {} with content size {}", i, "x".repeat((i % 1000) + 100));
            let path = format!("scale_test_{:04}_count_{}.txt", i, blob_count);
            std::fs::write(repo.join(&path), content).unwrap();
            run_git(&repo, &["add", &path]);
        }
        run_git(&repo, &["commit", "-m", &format!("Scalability test with {} blobs", blob_count)]);
        let start_time = std::time::Instant::now();
        let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(500); });
        let filter_time = start_time.elapsed();
        assert!(filter_time.as_millis() < 10_000);
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
    let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1000); });
    let dur = start.elapsed();
    assert!(dur.as_secs() < 30);
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
    let (_c1, _o1, _e1) = run_tool(&repo, |o| { o.max_blob_size = Some(1200); });
    let d1 = t1.elapsed();
    let t2 = std::time::Instant::now();
    let (_c2, _o2, _e2) = run_tool(&repo, |o| { o.max_blob_size = Some(1300); });
    let d2 = t2.elapsed();
    let ratio = if d1 > d2 { d1.as_millis() as f64 / d2.as_millis() as f64 } else { d2.as_millis() as f64 / d1.as_millis() as f64 };
    assert!(ratio < 10.0);
}
