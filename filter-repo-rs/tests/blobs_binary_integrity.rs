use filter_repo_rs as fr;

mod common;
use common::*;

#[test]
fn binary_not_modified_when_no_text_match() {
    let repo = init_repo();

    // Create a clearly binary file (contains NULs and non-UTF8 bytes)
    let bin_path = repo.join("bin.dat");
    let data: Vec<u8> = vec![0x00, 0xFF, 0x00, b'a', b'b', b'c', 0x00, 0x10, 0x9F];
    std::fs::write(&bin_path, &data).unwrap();
    run_git(&repo, &["add", "bin.dat"]);
    run_git(&repo, &["commit", "-m", "add binary file"]);

    // Record blob OID before rewriting
    let (_c1, oid_before, _e1) = run_git(&repo, &["rev-parse", "HEAD:bin.dat"]);
    let oid_before = oid_before.trim().to_string();

    // Replacement rules that do not occur in the binary payload
    let rules_file = repo.join("text_rules.txt");
    std::fs::write(&rules_file, "q==>X\nxyz==>XYZ\n").unwrap();

    let mut opts = fr::Options::default();
    opts.replace_text_file = Some(rules_file);
    opts.source = repo.clone();
    opts.target = repo.clone();
    opts.force = true;

    let result = fr::run(&opts);
    assert!(result.is_ok());

    // Record blob OID after rewriting; must be identical
    let (_c2, oid_after, _e2) = run_git(&repo, &["rev-parse", "HEAD:bin.dat"]);
    let oid_after = oid_after.trim().to_string();
    assert_eq!(
        oid_before, oid_after,
        "binary blob OID should be unchanged when no rules match"
    );
}
