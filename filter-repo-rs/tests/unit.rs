use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn unit_test_commit_message_processing() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Original commit message"]);
    let message_file = repo.join("message_replacements.txt");
    std::fs::write(&message_file, "Original==>Replacement").unwrap();
    let mut opts = fr::Options::default();
    opts.replace_message_file = Some(message_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, log, _e) = run_git(&repo, &["log", "--oneline", "-1"]);
    assert!(log.contains("Replacement"));
    assert!(!log.contains("Original"));
}

#[test]
fn unit_test_tag_processing() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test content").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Test commit for tags"]);
    run_git(&repo, &["tag", "lightweight-tag"]);
    run_git(
        &repo,
        &["tag", "-a", "annotated-tag", "-m", "Annotated tag message"],
    );
    let mut opts = fr::Options::default();
    opts.tag_rename = Some((b"lightweight-".to_vec(), b"renamed-lightweight-".to_vec()));
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.refs = vec!["--all".to_string()];
    let result = fr::run(&opts);
    assert!(result.is_ok());
    let (_c, tags, _e) = run_git(&repo, &["tag", "-l"]);
    let tags_list: Vec<&str> = tags.split('\n').collect();
    assert!(tags_list.contains(&"renamed-lightweight-tag"));
    assert!(!tags_list.contains(&"lightweight-tag"));
    assert!(tags_list.contains(&"annotated-tag"));
}

#[test]
fn unit_test_path_utilities() {
    use filter_repo_rs::pathutil;
    let unquoted = b"test\npath\tab";
    let dequoted = pathutil::dequote_c_style_bytes(unquoted);
    assert_eq!(dequoted, b"test\npath\tab");
    let unquoted = b"regular_path";
    let result = pathutil::dequote_c_style_bytes(unquoted);
    assert_eq!(result, unquoted);
    let empty = b"";
    let result = pathutil::dequote_c_style_bytes(empty);
    assert_eq!(result, empty);
}

#[test]
fn unit_test_git_utilities() {
    let repo = init_repo();
    std::fs::write(repo.join("test.txt"), "test").unwrap();
    run_git(&repo, &["add", "test.txt"]);
    run_git(&repo, &["commit", "-m", "Test commit"]);
    let (_c, head_ref_out, _e) = run_git(&repo, &["symbolic-ref", "HEAD"]);
    let head_ref = head_ref_out.trim();
    let (_c, output, _e) = run_git(&repo, &["show-ref", head_ref]);
    assert!(!output.is_empty());
}
