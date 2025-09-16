use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use filter_repo_rs as fr;

fn mktemp(prefix: &str) -> PathBuf {
  // Place temp repos under target/ to avoid Windows safe.directory issues
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.push("target"); p.push("it");
  static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
  let pid = std::process::id();
  let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
  let c = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  p.push(format!("{}_{}_{}_{}", prefix, pid, t, c));
  p
}

fn run_git(dir: &Path, args: &[&str]) -> (i32, String, String) {
  let out = Command::new("git").current_dir(dir).args(args).output().expect("run git");
  let code = out.status.code().unwrap_or(-1);
  let stdout = String::from_utf8_lossy(&out.stdout).to_string();
  let stderr = String::from_utf8_lossy(&out.stderr).to_string();
  (code, stdout, stderr)
}

fn write_file(dir: &Path, rel: &str, contents: &str) {
  let path = dir.join(rel);
  if let Some(p) = path.parent() { fs::create_dir_all(p).unwrap(); }
  let mut f = File::create(&path).unwrap();
  f.write_all(contents.as_bytes()).unwrap();
}

fn init_repo() -> PathBuf {
  let repo = mktemp("fr_rs_it");
  fs::create_dir_all(&repo).unwrap();
  // plain init (avoid optional flags for maximum compatibility)
  let (c, _o, e) = run_git(&repo, &["init"]);
  assert_eq!(c, 0, "git init failed: {}", e);
  run_git(&repo, &["config", "user.name", "A U Thor"]).0;
  run_git(&repo, &["config", "user.email", "a.u.thor@example.com"]).0;
  write_file(&repo, "README.md", "hello");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "init commit"]).0, 0);
  repo
}

fn run_tool(dir: &Path, configure: impl FnOnce(&mut fr::Options)) -> (i32, String, String) {
  let mut opts = fr::Options::default();
  opts.source = dir.to_path_buf();
  opts.target = dir.to_path_buf();
  configure(&mut opts);
  let res = fr::run(&opts);
  let code = if res.is_ok() { 0 } else { 1 };
  (code, String::new(), String::new())
}

#[test]
fn tag_rename_lightweight_creates_new_and_deletes_old() {
  let repo = init_repo();
  // create lightweight tag
  assert_eq!(run_git(&repo, &["tag", "v1.0"]).0, 0);
  // run rename
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec())); });
  // verify new exists
  let (_c2, out, _e2) = run_git(&repo, &["show-ref", "--tags"]);
  assert!(out.contains("refs/tags/release-1.0"), "expected release-1.0 in tags, got: {}", out);
  assert!(!out.contains("refs/tags/v1.0"), "old tag v1.0 should be deleted, got: {}", out);
}

#[test]
fn tag_rename_annotated_produces_tag_object() {
  let repo = init_repo();
  // annotated tag
  assert_eq!(run_git(&repo, &["tag", "-a", "-m", "hello tag", "v1.0"]).0, 0);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec())); });
  // resolve new tag object and check type
  let (_c1, oid, _e1) = run_git(&repo, &["rev-parse", "refs/tags/release-1.0"]);
  let oid = oid.trim();
  let (_c2, typ, _e2) = run_git(&repo, &["cat-file", "-t", oid]);
  assert_eq!(typ.trim(), "tag", "expected annotated tag object, got type {} for {}", typ, oid);
}

#[test]
fn replace_message_edits_commit_and_tag_messages() {
  let repo = init_repo();
  // second commit with token 'FOO'
  write_file(&repo, "src/a.txt", "x");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "commit with FOO token"]).0, 0);
  // annotated tag with token 'FOO'
  assert_eq!(run_git(&repo, &["tag", "-a", "-m", "tag msg FOO", "v2.0"]).0, 0);
  // replacement file
  let repl = repo.join("repl.txt");
  fs::write(&repl, "FOO==>BAR\n").unwrap();
  // run tool
  let (_c, _o, _e) = run_tool(&repo, |o| { o.replace_message_file = Some(repl.clone()); o.no_data = true; });
  // check HEAD message
  let (_c1, msg, _e1) = run_git(&repo, &["log", "-1", "--format=%B"]);
  assert!(msg.contains("BAR"), "expected commit message to contain BAR, got: {}", msg);
  assert!(!msg.contains("FOO"), "commit message should be rewritten, got: {}", msg);
  // check tag message
  let (_c2, tag_oid, _e2) = run_git(&repo, &["rev-parse", "refs/tags/v2.0"]);
  let tag_oid = tag_oid.trim();
  let (_c3, tag_obj, _e3) = run_git(&repo, &["cat-file", "-p", tag_oid]);
  assert!(tag_obj.contains("BAR"), "expected tag message to contain BAR, got: {}", tag_obj);
}

#[test]
fn writes_commit_map_and_ref_map() {
  let repo = init_repo();
  // annotated tag for ref-map
  run_git(&repo, &["tag", "-a", "-m", "msg", "v3.0"]);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; o.tag_rename = Some((b"v".to_vec(), b"release-".to_vec())); });
  let debug = repo.join(".git").join("filter-repo");
  let commit_map = debug.join("commit-map");
  let ref_map = debug.join("ref-map");
  assert!(commit_map.exists(), "commit-map should exist at {:?}", commit_map);
  // commit-map should be non-empty
  let mut s = String::new(); File::open(&commit_map).unwrap().read_to_string(&mut s).unwrap();
  assert!(!s.trim().is_empty(), "commit-map should have content");
  // ref-map should contain tag rename
  let mut r = String::new(); File::open(&ref_map).unwrap().read_to_string(&mut r).unwrap();
  assert!(r.contains("refs/tags/v3.0 refs/tags/release-3.0"), "ref-map expected v3.0->release-3.0, got: {}", r);
}

#[test]
fn branch_rename_updates_ref_and_head() {
  let repo = init_repo();
  // Determine current HEAD ref name
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  let headref = headref.trim().to_string(); // e.g., refs/heads/master
  // Perform branch rename: prefix '' -> 'renamed-'
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.branch_rename = Some((Vec::new(), b"renamed-".to_vec()));
    o.no_data = true;
  });
  // Verify new branch exists and old deleted
  let orig_name = headref.strip_prefix("refs/heads/").unwrap_or(&headref);
  let new_branch = format!("refs/heads/renamed-{}", orig_name);
  let (_c1, out1, _e1) = run_git(&repo, &["show-ref", "--verify", &new_branch]);
  assert!(!out1.is_empty(), "expected new branch to exist: {}", new_branch);
  let (_c2, out2, _e2) = run_git(&repo, &["show-ref", "--verify", &headref]);
  assert!(out2.is_empty(), "expected old branch to be deleted: {}", headref);
  // Verify HEAD points to new branch
  let (_c3, head_after, _e3) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(head_after.trim(), new_branch, "expected HEAD to follow renamed branch");
}

#[test]
fn branch_prefix_rename_preserves_head_to_mapped_target() {
  let repo = init_repo();
  // Create and switch to a prefixed branch
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
  write_file(&repo, "feat.txt", "feat");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "feat commit"]).0, 0);
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(headref.trim(), "refs/heads/features/foo");
  // Rename prefix features/ -> topics/
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
    o.no_data = true;
  });
  // New branch exists, old deleted
  let (_c1, out1, _e1) = run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/foo"]);
  assert!(!out1.is_empty(), "expected refs/heads/topics/foo to exist");
  let (_c2, out2, _e2) = run_git(&repo, &["show-ref", "--verify", "refs/heads/features/foo"]);
  assert!(out2.is_empty(), "expected refs/heads/features/foo to be deleted");
  // HEAD moved to mapped target
  let (_c3, head_after, _e3) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(head_after.trim(), "refs/heads/topics/foo", "expected HEAD to follow mapped branch");
}

#[test]
fn head_preserved_when_branch_unchanged() {
  let repo = init_repo();
  // Create another branch but keep HEAD on default branch
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  let headref = headref.trim().to_string();
  assert!(headref.starts_with("refs/heads/"));
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "feature/x"]).0, 0);
  // Switch back to original HEAD branch
  assert_eq!(run_git(&repo, &["checkout", "-q", headref.strip_prefix("refs/heads/").unwrap_or(&headref)]).0, 0);
  // Rename a different prefix that doesn't match current HEAD branch
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.branch_rename = Some((b"feature/".to_vec(), b"topic/".to_vec()));
    o.no_data = true;
  });
  // HEAD should remain the same
  let (_c1, head_after, _e1) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(head_after.trim(), headref, "expected HEAD to be preserved");
}

#[test]
fn multi_branch_prefix_rename_maps_all_and_preserves_others() {
  let repo = init_repo();
  // Determine default branch name
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  let headref = headref.trim().to_string();
  let def_short = headref.strip_prefix("refs/heads/").unwrap_or(&headref).to_string();

  // Create branches features/foo and features/bar with commits
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
  write_file(&repo, "f-foo.txt", "foo"); run_git(&repo, &["add", "."]).0; run_git(&repo, &["commit", "-q", "-m", "feat foo"]).0;
  assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/bar"]).0, 0);
  write_file(&repo, "f-bar.txt", "bar"); run_git(&repo, &["add", "."]).0; run_git(&repo, &["commit", "-q", "-m", "feat bar"]).0;

  // Another branch that should remain unchanged
  assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "misc/baz"]).0, 0);
  write_file(&repo, "baz.txt", "baz"); run_git(&repo, &["add", "."]).0; run_git(&repo, &["commit", "-q", "-m", "misc baz"]).0;

  // Apply branch prefix rename: features/ -> topics/
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
    o.no_data = true;
  });

  // Both features/* moved to topics/*
  let (_c1, out_topics_foo, _e1) = run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/foo"]);
  assert!(!out_topics_foo.is_empty(), "expected refs/heads/topics/foo to exist");
  let (_c2, out_topics_bar, _e2) = run_git(&repo, &["show-ref", "--verify", "refs/heads/topics/bar"]);
  assert!(!out_topics_bar.is_empty(), "expected refs/heads/topics/bar to exist");

  // Old branches deleted
  let (_c3, out_features_foo, _e3) = run_git(&repo, &["show-ref", "--verify", "refs/heads/features/foo"]);
  assert!(out_features_foo.is_empty(), "expected refs/heads/features/foo to be deleted");
  let (_c4, out_features_bar, _e4) = run_git(&repo, &["show-ref", "--verify", "refs/heads/features/bar"]);
  assert!(out_features_bar.is_empty(), "expected refs/heads/features/bar to be deleted");

  // Unrelated branch intact
  let (_c5, out_misc_baz, _e5) = run_git(&repo, &["show-ref", "--verify", "refs/heads/misc/baz"]);
  assert!(!out_misc_baz.is_empty(), "expected refs/heads/misc/baz to remain");
}

#[test]
fn multi_branch_prefix_rename_maps_head_from_deleted_branch() {
  let repo = init_repo();
  // Determine default branch name
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  let headref = headref.trim().to_string();
  let def_short = headref.strip_prefix("refs/heads/").unwrap_or(&headref).to_string();

  // Create features/foo and features/bar, move HEAD to features/bar
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/foo"]).0, 0);
  write_file(&repo, "f-foo.txt", "foo"); run_git(&repo, &["add", "."]).0; run_git(&repo, &["commit", "-q", "-m", "feat foo"]).0;
  assert_eq!(run_git(&repo, &["checkout", "-q", &def_short]).0, 0);
  assert_eq!(run_git(&repo, &["checkout", "-q", "-b", "features/bar"]).0, 0);
  write_file(&repo, "f-bar.txt", "bar"); run_git(&repo, &["add", "."]).0; run_git(&repo, &["commit", "-q", "-m", "feat bar"]).0;

  // Verify HEAD is features/bar
  let (_c_h, head_before, _e_h) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(head_before.trim(), "refs/heads/features/bar");

  // Apply rename features/ -> topics/
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.branch_rename = Some((b"features/".to_vec(), b"topics/".to_vec()));
    o.no_data = true;
  });

  // HEAD should map to topics/bar
  let (_c1, head_after, _e1) = run_git(&repo, &["symbolic-ref", "HEAD"]);
  assert_eq!(head_after.trim(), "refs/heads/topics/bar", "expected HEAD to map to renamed branch");
}

#[test]
fn max_blob_size_drops_large_blobs() {
  let repo = init_repo();
  // Create files of different sizes
  let big = vec![b'A'; 4096];
  let small = vec![b'B'; 10];
  fs::write(repo.join("big.bin"), &big).unwrap();
  fs::write(repo.join("small.bin"), &small).unwrap();
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add blobs"]).0, 0);
  // Run with max blob size threshold smaller than big.bin
  let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1024); o.no_data = false; });
  let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(tree.contains("small.bin"), "expected small.bin to remain, got: {}", tree);
  assert!(!tree.contains("big.bin"), "expected big.bin to be dropped, got: {}", tree);
}

#[test]
fn replace_text_redacts_blob_contents() {
  let repo = init_repo();
  // Create a file with a secret token
  write_file(&repo, "secret.txt", "token=SECRET-ABC-123\n");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add secret"]).0, 0);
  // Replacement file
  let repl = repo.join("repl-blobs.txt");
  fs::write(&repl, "SECRET-ABC-123==>REDACTED\n").unwrap();
  // Run tool with --replace-text
  let (_c, _o, _e) = run_tool(&repo, |o| { o.replace_text_file = Some(repl.clone()); o.no_data = false; });
  // Read back blob content via git show HEAD:secret.txt
  let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:secret.txt"]);
  assert!(content.contains("REDACTED"), "expected blob content to be redacted, got: {}", content);
  assert!(!content.contains("SECRET-ABC-123"), "expected original secret to be removed, got: {}", content);
}

#[test]
fn path_filter_includes_only_prefix() {
  let repo = init_repo();
  write_file(&repo, "src/keep.txt", "k");
  write_file(&repo, "docs/drop.txt", "d");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add files"]).0, 0);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.paths.push(b"src/".to_vec()); });
  // Read back with quoting disabled for human-readable output
  let (_c2, tree, _e2) = run_git(&repo, &["-c", "core.quotepath=false", "ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(tree.contains("src/keep.txt"), "expected to keep src/keep.txt, got: {}", tree);
  assert!(!tree.contains("docs/drop.txt"), "expected to drop docs/drop.txt, got: {}", tree);
}

#[test]
fn path_rename_applies_to_paths() {
  let repo = init_repo();
  write_file(&repo, "a/file.txt", "x");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add a/file.txt"]).0, 0);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.path_renames.push((b"a/".to_vec(), b"x/".to_vec())); });
  let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(tree.contains("x/file.txt"), "expected path renamed to x/file.txt, got: {}", tree);
  assert!(!tree.contains("a/file.txt"), "expected old path a/file.txt removed, got: {}", tree);
}

#[test]
fn path_glob_selects_md_under_src() {
  let repo = init_repo();
  write_file(&repo, "src/a.md", "m");
  write_file(&repo, "src/a.txt", "t");
  write_file(&repo, "src/deep/b.md", "m");
  write_file(&repo, "docs/x.md", "m");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add various files"]).0, 0);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.path_globs.push(b"src/**/*.md".to_vec()); });
  let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(tree.contains("src/a.md"), "expected to keep src/a.md, got: {}", tree);
  assert!(tree.contains("src/deep/b.md"), "expected to keep src/deep/b.md, got: {}", tree);
  assert!(!tree.contains("src/a.txt"), "expected to drop src/a.txt, got: {}", tree);
  assert!(!tree.contains("docs/x.md"), "expected to drop docs/x.md, got: {}", tree);
}

#[test]
fn invert_paths_drops_prefix() {
  let repo = init_repo();
  write_file(&repo, "src/keep.txt", "k");
  write_file(&repo, "drop/file.txt", "d");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "prepare files"]).0, 0);
  let (_c, _o, _e) = run_tool(&repo, |o| { o.paths.push(b"drop/".to_vec()); o.invert_paths = true; });
  let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(tree.contains("src/keep.txt"), "expected to keep src/keep.txt, got: {}", tree);
  assert!(!tree.contains("drop/file.txt"), "expected to drop drop/file.txt, got: {}", tree);
}

#[test]
fn quoted_paths_roundtrip_with_rename() {
  let repo = init_repo();
  // Force quoting in fast-export
  run_git(&repo, &["config", "core.quotepath", "true"]);
  // Create a filename with non-ASCII to trigger C-style quoting
  write_file(&repo, "src/ümlaut.txt", "u");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add umlaut"]).0, 0);
  // Move everything into X/ using to-subdirectory behavior; disable our quotepath override
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.quotepath = false; // do NOT pass -c core.quotepath=false
    o.path_renames.push((Vec::new(), b"X/".to_vec()));
    o.no_data = true;
  });
  let (_c2, tree, _e2) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  // Normalize by dequoting any C-style quoted entries
  let mut found = false;
  for line in tree.lines() {
    let s = line.trim(); if s.is_empty() { continue; }
    let norm = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
      let inner = &s.as_bytes()[1..s.as_bytes().len()-1];
      String::from_utf8_lossy(&fr::dequote_c_style_bytes(inner)).to_string()
    } else { s.to_string() };
    if norm == "X/src/ümlaut.txt" { found = true; break; }
  }
  assert!(found, "expected renamed quoted path to roundtrip, got: {}", tree);
}

#[test]
fn inline_replace_text_and_report_modified() {
  // use std::env as stdenv;
  let repo = init_repo();
  // Build a minimal fast-export-like stream that uses inline data in a commit
  let stream_path = repo.join("fe-inline.stream");
  let payload = "token=SECRET-INLINE-123\n";
  let payload_len = payload.as_bytes().len();
  let msg = "inline commit\n"; let msg_len = msg.as_bytes().len();
  let mut s = String::new();
  let (_hc, headref, _he) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
  let commit_ref = headref.trim();
  s.push_str(&format!("commit {}\n", commit_ref));
  s.push_str("mark :1\n");
  s.push_str("committer A U Thor <a.u.thor@example.com> 1737070000 +0000\n");
  s.push_str(&format!("data {}\n{}", msg_len, msg));
  s.push_str("M 100644 inline secret.txt\n");
  s.push_str(&format!("data {}\n{}", payload_len, payload));
  s.push_str("\n");
  s.push_str("done\n");
  std::fs::write(&stream_path, s).unwrap();

  // Replacement rules: redact the secret token
  let repl = repo.join("repl-inline.txt");
  std::fs::write(&repl, "SECRET-INLINE-123==>REDACTED\n").unwrap();

  // Force our tool to read the prebuilt stream via options override
  let (_c, _o, _e) = run_tool(&repo, |o| {
    o.replace_text_file = Some(repl.clone());
    o.no_data = false; o.write_report = true;
    // use internal test override
    #[allow(deprecated)]
    { o.fe_stream_override = Some(stream_path.clone()); }
  });

  // Verify blob content rewritten
  let (_cc, content, _ee) = run_git(&repo, &["show", "HEAD:secret.txt"]);
  assert!(content.contains("REDACTED"), "expected inline blob to be redacted, got: {}", content);
  assert!(!content.contains("SECRET-INLINE-123"), "expected original secret to be removed, got: {}", content);

  // Verify report includes modified count and path sample
  let report = repo.join(".git").join("filter-repo").join("report.txt");
  let mut s = String::new();
  std::fs::File::open(&report).unwrap().read_to_string(&mut s).unwrap();
  assert!(s.contains("Blobs modified by replace-text"), "expected modified counter in report, got: {}", s);
  assert!(s.contains("secret.txt"), "expected modified sample path in report, got: {}", s);
}
#[cfg(feature = "blob-regex")]
#[test]
fn replace_text_regex_redacts_blob() {
  let repo = init_repo();
  write_file(&repo, "data.txt", "foo123 foo999\n");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add data"]).0, 0);
  let repl = repo.join("repl-regex.txt");
  std::fs::write(&repl, "regex:foo[0-9]+==>X\n").unwrap();
  let (_c, _o, _e) = run_tool(&repo, |o| { o.replace_text_file = Some(repl.clone()); o.no_data = false; });
  let (_c2, content, _e2) = run_git(&repo, &["show", "HEAD:data.txt"]);
  assert!(content.contains("X X"), "expected both occurrences replaced, got: {}", content);
  assert!(!content.contains("foo123"), "expected original tokens removed, got: {}", content);
}

#[test]
fn strip_report_written() {
  let repo = init_repo();
  // create one small and one large file
  write_file(&repo, "small.txt", "x");
  // ~10KB large file
  let big_data = vec![b'A'; 10_000];
  let mut f = File::create(repo.join("big.bin")).unwrap();
  f.write_all(&big_data).unwrap();
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add files"]).0, 0);
  // run tool with small max-blob-size and report enabled
  let (_c, _o, _e) = run_tool(&repo, |o| { o.max_blob_size = Some(1024); o.write_report = true; });
  let report = repo.join(".git").join("filter-repo").join("report.txt");
  assert!(report.exists(), "expected report at {:?}", report);
  let mut s = String::new();
  File::open(&report).unwrap().read_to_string(&mut s).unwrap();
  assert!(s.contains("Blobs stripped by size"), "expected size counter in report, got: {}", s);
  assert!(s.contains("big.bin"), "expected sample path for size-stripped blob, got: {}", s);
}

#[test]
fn dry_run_does_not_modify_refs_or_remote() {
  let repo = init_repo();
  // Track HEAD and add a self origin
  let (_c0, head_before, _e0) = run_git(&repo, &["rev-parse", "HEAD"]);
  assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
  // Run with dry-run and write-report
  let (_c, _o, _e) = run_tool(&repo, |o| { o.dry_run = true; o.write_report = true; o.no_data = true; });
  // HEAD unchanged
  let (_c1, head_after, _e1) = run_git(&repo, &["rev-parse", "HEAD"]);
  assert_eq!(head_before.trim(), head_after.trim(), "expected HEAD unchanged in dry-run");
  // origin remote still exists
  let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
  assert!(remotes.contains("origin"), "expected origin to remain in dry-run, got: {}", remotes);
  // report exists
  let report = repo.join(".git").join("filter-repo").join("report.txt");
  assert!(report.exists(), "expected report in dry-run at {:?}", report);
}

#[test]
fn strip_ids_report_written() {
  let repo = init_repo();
  // Create a file and commit
  write_file(&repo, "secret.bin", "topsecret\n");
  run_git(&repo, &["add", "."]).0;
  assert_eq!(run_git(&repo, &["commit", "-q", "-m", "add secret.bin"]).0, 0);
  // Resolve blob id for HEAD:secret.bin
  let (_c0, blob_id, _e0) = run_git(&repo, &["rev-parse", "HEAD:secret.bin"]);
  let sha = blob_id.trim();
  // Write SHA to list file
  let shalist = repo.join("strip-sha.txt");
  std::fs::write(&shalist, format!("{}\n", sha)).unwrap();
  // Run tool with strip-blobs-with-ids and report enabled
  let (_c, _o, _e) = run_tool(&repo, |o| { o.strip_blobs_with_ids = Some(shalist.clone()); o.write_report = true; });
  // The file should be dropped from the tree
  let (_c1, tree, _e1) = run_git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
  assert!(!tree.contains("secret.bin"), "expected secret.bin to be dropped, got: {}", tree);
  // Report should include SHA count and sample path under sha section
  let report = repo.join(".git").join("filter-repo").join("report.txt");
  let mut s = String::new();
  std::fs::File::open(&report).unwrap().read_to_string(&mut s).unwrap();
  assert!(s.contains("Blobs stripped by SHA:"), "expected SHA counter in report, got: {}", s);
  assert!(s.contains("Sample paths (sha):") && s.contains("secret.bin"), "expected sha sample path in report, got: {}", s);
}

#[test]
fn partial_mode_keeps_origin_and_remote_tracking() {
  let repo = init_repo();
  // Determine current HEAD ref
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
  let headref = headref.trim();
  let branch = headref.strip_prefix("refs/heads/").unwrap_or(headref);
  // Add a self origin and create a remote-tracking ref
  assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
  let spec = format!("+{}:refs/remotes/origin/{}", headref, branch);
  assert_eq!(run_git(&repo, &["fetch", "origin", &spec]).0, 0);
  // Run tool with partial
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; o.partial = true; });
  // origin remote should remain
  let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
  assert!(remotes.contains("origin"), "expected origin remote to remain in partial mode, got: {}", remotes);
  // remote-tracking ref should remain
  let (c3, _o3, _e3) = run_git(&repo, &["show-ref", "--verify", &format!("refs/remotes/origin/{}", branch)]);
  assert_eq!(c3, 0, "expected remote-tracking ref to remain in partial mode");
}

#[test]
fn sensitive_fetch_all_from_bare_remote() {
  // Create a bare remote with an extra branch not present locally
  let bare = mktemp("fr_rs_bare");
  std::fs::create_dir_all(&bare).unwrap();
  assert_eq!(run_git(&bare, &["init", "--bare"]).0, 0);

  // Seed repo to push into bare
  let seed = mktemp("fr_rs_seed");
  std::fs::create_dir_all(&seed).unwrap();
  assert_eq!(run_git(&seed, &["init"]).0, 0);
  run_git(&seed, &["config", "user.name", "A U Thor"]).0;
  run_git(&seed, &["config", "user.email", "a.u.thor@example.com"]).0;
  write_file(&seed, "README.md", "seed");
  run_git(&seed, &["add", "."]).0;
  assert_eq!(run_git(&seed, &["commit", "-q", "-m", "seed init"]).0, 0);
  // Create an extra branch with a file
  assert_eq!(run_git(&seed, &["checkout", "-b", "extra"]).0, 0);
  write_file(&seed, "extra.txt", "x");
  run_git(&seed, &["add", "."]).0;
  assert_eq!(run_git(&seed, &["commit", "-q", "-m", "add extra"]).0, 0);
  // Add bare as origin and push all
  let bare_str = bare.to_string_lossy().to_string();
  assert_eq!(run_git(&seed, &["remote", "add", "origin", &bare_str]).0, 0);
  assert_eq!(run_git(&seed, &["push", "-q", "origin", "--all"]).0, 0);

  // Our working repo
  let repo = init_repo();
  // Add origin pointing to bare, but do not fetch
  assert_eq!(run_git(&repo, &["remote", "add", "origin", &bare_str]).0, 0);

  // Sanity: branch 'extra' should not exist locally yet
  let (c0, _o0, _e0) = run_git(&repo, &["show-ref", "--verify", "refs/heads/extra"]);
  assert_ne!(c0, 0, "extra branch should not exist before sensitive fetch");

  // Run tool in sensitive mode to trigger fetch-all (no --no-fetch)
  let (_c, _o, _e) = run_tool(&repo, |o| { o.sensitive = true; o.no_data = true; });

  // After run, 'extra' should exist due to fetch-all with empty refmap
  let (c1, _o1, _e1) = run_git(&repo, &["show-ref", "--verify", "refs/heads/extra"]);
  assert_eq!(c1, 0, "expected sensitive fetch-all to create refs/heads/extra");
  // origin remote should remain in sensitive mode
  let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
  assert!(remotes.contains("origin"), "expected origin to remain for sensitive mode");
}
#[test]
fn origin_migration_and_removal_nonsensitive() {
  let repo = init_repo();
  // Determine current HEAD ref
  let (_c0, headref, _e0) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
  let headref = headref.trim();
  let branch = headref.strip_prefix("refs/heads/").unwrap_or(headref);
  // Add a self origin and create a remote-tracking ref
  assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
  let spec = format!("+{}:refs/remotes/origin/{}", headref, branch);
  assert_eq!(run_git(&repo, &["fetch", "origin", &spec]).0, 0);
  // Verify remote-tracking exists
  let (_c1, out1, _e1) = run_git(&repo, &["show-ref", "--verify", &format!("refs/remotes/origin/{}", branch)]);
  assert!(out1.contains(&format!("refs/remotes/origin/{}", branch)), "expected remote-tracking ref created, got: {}", out1);
  // Run tool (non-sensitive, full)
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; });
  // Origin remote should be removed
  let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
  assert!(!remotes.contains("origin"), "expected origin remote removed, got: {}", remotes);
  // Remote-tracking ref should be gone
  let (c3, _o3, _e3) = run_git(&repo, &["show-ref", "--verify", &format!("refs/remotes/origin/{}", branch)]);
  assert_ne!(c3, 0, "expected remote-tracking ref removed");
  // Local branch should exist
  let (c4, _o4, _e4) = run_git(&repo, &["show-ref", "--verify", headref]);
  assert_eq!(c4, 0, "expected local branch to exist: {}", headref);
}

#[test]
fn sensitive_mode_keeps_origin_remote() {
  let repo = init_repo();
  // Ensure origin exists
  assert_eq!(run_git(&repo, &["remote", "add", "origin", "."]).0, 0);
  // Run tool with sensitive mode and no_fetch (avoid heavy operations)
  let (_c, _o, _e) = run_tool(&repo, |o| { o.no_data = true; o.sensitive = true; o.no_fetch = true; });
  // Origin remote should remain
  let (_c2, remotes, _e2) = run_git(&repo, &["remote"]);
  assert!(remotes.contains("origin"), "expected origin remote to remain in sensitive mode, got: {}", remotes);
}
