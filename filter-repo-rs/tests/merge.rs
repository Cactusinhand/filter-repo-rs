use std::fs;

mod common;
use common::*;

#[test]
fn merge_parents_dedup_when_side_branch_pruned() {
    let repo = init_repo();
    let base_branch = current_branch(&repo);

    write_file(&repo, "keep.txt", "base");
    assert_eq!(run_git(&repo, &["add", "keep.txt"]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "add keep path"]).0, 0);

    assert_eq!(run_git(&repo, &["checkout", "-b", "feature-branch"]).0, 0);
    write_file(&repo, "drop.txt", "side change");
    assert_eq!(run_git(&repo, &["add", "drop.txt"]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "side branch change"]).0, 0);

    assert_eq!(run_git(&repo, &["checkout", &base_branch]).0, 0);
    assert_eq!(
        run_git(&repo, &["merge", "--no-ff", "--no-commit", "feature-branch"]).0,
        0,
        "expected merge to succeed",
    );
    write_file(&repo, "keep.txt", "merge resolution");
    assert_eq!(run_git(&repo, &["add", "keep.txt"]).0, 0);
    assert_eq!(run_git(&repo, &["commit", "-m", "merge feature branch"]).0, 0);

    let (code, _, _) = run_tool(&repo, |opts| {
        opts.paths.push(b"keep.txt".to_vec());
    });
    assert_eq!(code, 0, "filter run should succeed");

    let filtered_path = repo
        .join(".git")
        .join("filter-repo")
        .join("fast-export.filtered");
    let data = fs::read(&filtered_path).expect("read filtered fast-export");
    let marker = b"merge feature branch";
    let pos = data
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("merge commit message present");
    let commit_tag = b"\ncommit ";
    let commit_start = data[..pos]
        .windows(commit_tag.len())
        .rposition(|w| w == commit_tag)
        .map(|idx| idx + 1)
        .unwrap_or(0);

    let mut idx = commit_start;
    let mut parents: Vec<Vec<u8>> = Vec::new();
    while idx < data.len() {
        let line_end = match data[idx..].iter().position(|&b| b == b'\n') {
            Some(end) => idx + end,
            None => break,
        };
        let line = &data[idx..line_end];
        idx = line_end + 1;
        if line.is_empty() {
            break;
        }
        if line.starts_with(b"data ") {
            let len = std::str::from_utf8(&line[b"data ".len()..])
                .expect("utf8 data header")
                .trim()
                .parse::<usize>()
                .expect("numeric data length");
            idx = idx.saturating_add(len);
            if idx < data.len() && data[idx] == b'\n' {
                idx += 1;
            }
            continue;
        }
        if line.starts_with(b"from ") || line.starts_with(b"merge ") {
            parents.push(line.to_vec());
        }
    }

    assert_eq!(parents.len(), 1, "expected single parent line: {:?}", parents);
    assert!(parents[0].starts_with(b"from :"));
}

