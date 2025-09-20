use std::fs;
use std::path::{Path, PathBuf};

mod common;
use common::*;

fn find_bundles_in(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .expect("failed to read backup directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("bundle") {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn backup_creates_bundle_in_filter_repo_directory() {
    let repo = init_repo();
    run_tool_expect_success(&repo, |o| {
        o.backup = true;
        o.no_data = true;
    });

    let backup_dir = repo.join(".git").join("filter-repo");
    assert!(
        backup_dir.exists(),
        "backup directory should exist at {:?}",
        backup_dir
    );

    let bundles = find_bundles_in(&backup_dir);
    assert!(
        !bundles.is_empty(),
        "expected at least one bundle in {:?}, entries: {:?}",
        backup_dir,
        bundles
    );
}

#[test]
fn backup_respects_directory_override() {
    let repo = init_repo();
    let custom_dir = PathBuf::from("custom-backups");
    run_tool_expect_success(&repo, |o| {
        o.backup = true;
        o.no_data = true;
        o.backup_path = Some(custom_dir.clone());
    });

    let backup_dir = repo.join(&custom_dir);
    assert!(
        backup_dir.exists(),
        "backup directory should exist at {:?}",
        backup_dir
    );

    let bundles = find_bundles_in(&backup_dir);
    assert!(
        !bundles.is_empty(),
        "expected at least one bundle in {:?}, entries: {:?}",
        backup_dir,
        bundles
    );
}

#[test]
fn backup_honors_explicit_file_path() {
    let repo = init_repo();
    let rel_path = PathBuf::from("custom/custom-bundle.bundle");
    let expected_path = repo.join(&rel_path);
    run_tool_expect_success(&repo, |o| {
        o.backup = true;
        o.no_data = true;
        o.backup_path = Some(rel_path.clone());
    });

    assert!(
        expected_path.exists(),
        "expected bundle to exist at {:?}",
        expected_path
    );
}
