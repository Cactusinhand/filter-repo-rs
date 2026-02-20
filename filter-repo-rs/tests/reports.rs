use std::fs::File;
use std::io::Read;

mod common;
use common::*;

#[test]
fn strip_report_written() {
    let repo = init_repo();
    write_file(&repo, "small.txt", "x");
    let big_data = vec![b'A'; 10_000];
    let mut f = File::create(repo.join("big.bin")).unwrap();
    use std::io::Write as _;
    f.write_all(&big_data).unwrap();
    f.flush().unwrap();
    drop(f);
    run_git(&repo, &["add", "."]);
    assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add files"]).0, 0);
    run_tool_expect_success(&repo, |o| {
        o.max_blob_size = Some(1024);
        o.write_report = true;
    });
    let (_c2, tree_after, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree_after.contains("big.bin"));
    assert!(tree_after.contains("small.txt"));
    let report = repo.join(".git").join("filter-repo").join("report.txt");
    assert!(report.exists());
    let mut s = String::new();
    File::open(&report).unwrap().read_to_string(&mut s).unwrap();
    assert!(s.contains("Blobs stripped by size"));
}

#[test]
fn strip_ids_report_written() {
    let repo = init_repo();
    write_file(&repo, "secret.bin", "topsecret\n");
    run_git(&repo, &["add", "."]);
    assert_eq!(
        run_git(&repo, &["commit", "-q", "-m", "add secret.bin"]).0,
        0
    );
    let (_c0, blob_id, _e0) = run_git(&repo, &["rev-parse", "HEAD:secret.bin"]);
    let sha = blob_id.trim();
    let shalist = repo.join("strip-sha.txt");
    std::fs::write(&shalist, format!("{}\n", sha)).unwrap();
    run_tool_expect_success(&repo, |o| {
        o.strip_blobs_with_ids = Some(shalist.clone());
        o.write_report = true;
    });
    let (_c1, tree, _e1) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(!tree.contains("secret.bin"));
    let report = repo.join(".git").join("filter-repo").join("report.txt");
    let mut s = String::new();
    File::open(&report).unwrap().read_to_string(&mut s).unwrap();
    assert!(s.contains("Blobs stripped by SHA:"));
    assert!(s.contains("secret.bin"));
}

#[cfg(windows)]
#[test]
fn windows_path_report_is_written_even_without_write_report() {
    let repo = init_repo();
    let stream_path = repo.join("fe-windows-path-report.stream");
    let stream = r#"blob
mark :1
data 1
x

commit refs/heads/main
mark :2
author Tester <tester@example.com> 0 +0000
committer Tester <tester@example.com> 0 +0000
data 3
c1
M 100644 :1 "bad:name?.txt "

done
"#;
    std::fs::write(&stream_path, stream).expect("write windows path report stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Sanitize;
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    });

    let path_report = repo
        .join(".git")
        .join("filter-repo")
        .join("windows-path-report.txt");
    assert!(
        path_report.exists(),
        "windows path report should be generated when policy has hits"
    );
    let mut s = String::new();
    File::open(path_report)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s.contains("Policy: sanitize"), "missing policy line: {}", s);
    assert!(
        s.contains("bad:name?.txt"),
        "missing original path sample: {}",
        s
    );
}

#[cfg(windows)]
#[test]
fn windows_path_summary_is_included_in_text_and_json_reports() {
    let repo = init_repo();
    let stream_path = repo.join("fe-windows-path-summary.stream");
    let stream = r#"blob
mark :1
data 1
x

commit refs/heads/main
mark :2
author Tester <tester@example.com> 0 +0000
committer Tester <tester@example.com> 0 +0000
data 3
c1
M 100644 :1 "bad:name?.txt "

done
"#;
    std::fs::write(&stream_path, stream).expect("write windows path summary stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.write_report = true;
        o.write_report_json = true;
        o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Sanitize;
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    });

    let report_txt = repo.join(".git").join("filter-repo").join("report.txt");
    let mut txt = String::new();
    File::open(report_txt)
        .unwrap()
        .read_to_string(&mut txt)
        .unwrap();
    assert!(
        txt.contains("Windows path compatibility"),
        "text report should include windows path summary: {}",
        txt
    );

    let report_json = repo.join(".git").join("filter-repo").join("report.json");
    let mut json = String::new();
    File::open(report_json)
        .unwrap()
        .read_to_string(&mut json)
        .unwrap();
    assert!(
        json.contains("\"windows_path\""),
        "json report should include windows_path section: {}",
        json
    );
}
