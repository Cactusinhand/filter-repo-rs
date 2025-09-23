use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn analyze_mode_produces_human_report() {
    let repo = init_repo();
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.mode = fr::Mode::Analyze;
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    let report = fr::analysis::generate_report(&opts).expect("generate analysis report");
    assert!(
        report.metrics.refs_total >= 1,
        "expected refs to be counted"
    );
    assert!(
        !report.warnings.is_empty(),
        "expected at least one informational warning"
    );
    fr::analysis::run(&opts).expect("analyze mode should render without error");
}

#[test]
fn analyze_mode_emits_json() {
    let repo = init_repo();
    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.mode = fr::Mode::Analyze;
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    let report = fr::analysis::generate_report(&opts).expect("generate analysis report");
    let json = serde_json::to_string(&report).expect("serialize report");
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert!(
        v.get("metrics").is_some(),
        "metrics missing in json: {}",
        json
    );
    assert!(
        v.get("warnings").is_some(),
        "warnings missing in json: {}",
        json
    );
    opts.analyze.json = true;
    fr::analysis::run(&opts).expect("json analyze run should succeed");
}

#[test]
fn analyze_mode_limits_top_entries_and_populates_paths() {
    let repo = init_repo();
    // create blobs of various sizes so the top list can be truncated
    for i in 0..5 {
        let size = (i + 1) * 1024;
        let contents = "x".repeat(size);
        write_file(&repo, &format!("data/blob{}.bin", i), &contents);
    }
    // create multiple duplicate blobs with distinct contents to ensure truncation
    for (idx, paths) in [
        ("A", vec!["dups/a1.txt", "dups/a2.txt", "dups/a3.txt"]),
        ("B", vec!["dups/b1.txt", "dups/b2.txt"]),
        ("C", vec!["dups/c1.txt", "dups/c2.txt"]),
    ] {
        let payload = format!("duplicate payload {}", idx);
        for path in paths {
            write_file(&repo, path, &payload);
        }
    }
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "populate blobs"]).0, 0);

    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.mode = fr::Mode::Analyze;
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.analyze.top = 2;
    opts.analyze.thresholds.warn_blob_bytes = 1500;
    let report = fr::analysis::generate_report(&opts).expect("generate analysis report");

    assert!(
        report.metrics.largest_blobs.len() <= opts.analyze.top,
        "largest blobs exceeded top limit"
    );
    assert!(
        report.metrics.blobs_over_threshold.len() <= opts.analyze.top,
        "threshold hits exceeded top limit"
    );
    assert!(
        report.metrics.duplicate_blobs.len() <= opts.analyze.top,
        "duplicate blob list exceeded top limit"
    );
    assert!(
        report
            .metrics
            .largest_blobs
            .iter()
            .all(|b| b.path.is_some()),
        "expected sample paths for top blobs"
    );
    assert!(
        report
            .metrics
            .blobs_over_threshold
            .iter()
            .all(|b| b.path.is_some()),
        "expected sample paths for threshold hits"
    );
    assert!(
        report
            .metrics
            .duplicate_blobs
            .iter()
            .all(|d| d.example_path.is_some()),
        "expected example paths for duplicates"
    );
}

#[test]
fn analyze_mode_warns_on_commit_thresholds() {
    let repo = init_repo();
    // oversized commit message that should exceed the configured threshold
    write_file(&repo, "logs.txt", &"L".repeat(64));
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", &"M".repeat(64)]).0, 0);
    let (_, long_oid, _) = run_git(&repo, &["rev-parse", "HEAD"]);
    let long_oid = long_oid.trim().to_string();
    // determine the name of the default branch (e.g. master or main)
    // prefer symbolic-ref, but fall back to rev-parse if needed
    let (_, base_branch, _) = run_git(&repo, &["symbolic-ref", "--short", "HEAD"]);
    let mut base_branch = base_branch.trim().to_string();
    if base_branch.is_empty() || base_branch == "HEAD" {
        let (_, alt, _) = run_git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        base_branch = alt.trim().to_string();
    }

    // create a feature branch and diverging history to produce a merge commit
    assert_eq!(run_git(&repo, &["checkout", "-b", "feature"]).0, 0);
    write_file(&repo, "feature.txt", "feature work");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "feature commit"]).0, 0);

    // return to the original default branch, regardless of its name
    assert_eq!(run_git(&repo, &["checkout", &base_branch]).0, 0);
    write_file(&repo, "master.txt", "master work");
    assert_eq!(run_git(&repo, &["add", "."]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "master commit"]).0, 0);

    let merge_msg = "Merge branch 'feature' with an explanation that exceeds the warn threshold";
    assert_eq!(run_git(&repo, &["merge", "feature", "-m", merge_msg]).0, 0);

    let mut opts = fr::Options::default();
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.mode = fr::Mode::Analyze;
    opts.force = true; // Use --force to bypass sanity checks for unit tests
    opts.analyze.thresholds.warn_commit_msg_bytes = 32;
    opts.analyze.thresholds.warn_max_parents = 1;

    let report = fr::analysis::generate_report(&opts).expect("generate analysis report");

    assert!(
        report.metrics.max_commit_parents > 1,
        "expected merge commit to exceed parent threshold"
    );
    assert!(
        report
            .metrics
            .oversized_commit_messages
            .iter()
            .any(|m| m.oid.trim() == long_oid),
        "expected long commit message to be recorded"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains(&long_oid)),
        "expected warning mentioning oversized commit message"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains("parents")),
        "expected warning about excessive commit parents"
    );
}
