use std::io::Read;

mod common;
use common::*;

#[test]
fn rename_and_copy_paths_requote_after_filtering() {
    let repo = init_repo();
    let stream_path = repo.join("fe-renames.stream");
    let stream = r#"blob
mark :1
data 4
one

commit refs/heads/main
mark :2
author Tester <tester@example.com> 0 +0000
committer Tester <tester@example.com> 0 +0000
data 3
c1
M 100644 :1 "sp ace.txt"
M 100644 :1 "old\001.txt"
M 100644 :1 "removed space.txt"

commit refs/heads/main
mark :3
author Tester <tester@example.com> 1 +0000
committer Tester <tester@example.com> 1 +0000
data 3
c2
from :2
D "removed space.txt"
C "sp ace.txt" "dup space.txt"
R "old\001.txt" "final\001name.txt"

done
"#;
    std::fs::write(&stream_path, stream).expect("write custom fast-export stream");

    run_tool_expect_success(&repo, |o| {
        o.dry_run = true;
        o.path_renames.push((Vec::new(), b"prefix/".to_vec()));
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    });

    let filtered_path = repo
        .join(".git")
        .join("filter-repo")
        .join("fast-export.filtered");
    let filtered = std::fs::read_to_string(&filtered_path).expect("read filtered stream");

    assert!(filtered.contains("M 100644 :1 \"prefix/sp ace.txt\""));
    assert!(filtered.contains("M 100644 :1 \"prefix/old\\001.txt\""));
    assert!(filtered.contains("D \"prefix/removed space.txt\""));
    assert!(filtered.contains("C \"prefix/sp ace.txt\" \"prefix/dup space.txt\""));
    assert!(filtered.contains("R \"prefix/old\\001.txt\" \"prefix/final\\001name.txt\""));
}

#[test]
fn inline_replace_text_and_report_modified() {
    let repo = init_repo();
    let stream_path = repo.join("fe-inline.stream");
    let payload = "token=SECRET-INLINE-123\n";
    let payload_len = payload.as_bytes().len();
    let msg = "inline commit\n";
    let msg_len = msg.as_bytes().len();
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

    let repl = repo.join("repl-inline.txt");
    std::fs::write(&repl, "SECRET-INLINE-123==>REDACTED\n").unwrap();

    run_tool_expect_success(&repo, |o| {
        o.replace_text_file = Some(repl.clone());
        o.no_data = false;
        o.write_report = true;
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    });

    let (_cc, content, _ee) = run_git(&repo, &["show", "HEAD:secret.txt"]);
    assert!(content.contains("REDACTED"));
    assert!(!content.contains("SECRET-INLINE-123"));

    let report = repo.join(".git").join("filter-repo").join("report.txt");
    let mut s = String::new();
    std::fs::File::open(&report)
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s.contains("Blobs modified by replace-text"));
    assert!(s.contains("secret.txt"));
}
