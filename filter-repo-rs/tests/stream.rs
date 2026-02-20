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
        o.debug_mode = true;
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
    assert!(filtered.contains("M 100644 :1 prefix/old_.txt"));
    assert!(filtered.contains("D \"prefix/removed space.txt\""));
    assert!(filtered.contains("C \"prefix/sp ace.txt\" \"prefix/dup space.txt\""));
    assert!(filtered.contains("R prefix/old_.txt prefix/final_name.txt"));
}

#[test]
fn rename_and_copy_paths_requote_handles_escaped_quotes_backslashes_and_octal_utf8() {
    let repo = init_repo();
    let stream_path = repo.join("fe-renames-quoted.stream");
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
M 100644 :1 "src/quo\"te\\caf\303\251.txt"

commit refs/heads/main
mark :3
author Tester <tester@example.com> 1 +0000
committer Tester <tester@example.com> 1 +0000
data 3
c2
from :2
C "src/quo\"te\\caf\303\251.txt" "src/dup\"te\\caf\303\251.txt"
R "src/quo\"te\\caf\303\251.txt" "src/fin\"al\\caf\303\251.txt"
D "src/dup\"te\\caf\303\251.txt"

done
"#;
    std::fs::write(&stream_path, stream).expect("write quoted fast-export stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.path_renames.push((b"src/".to_vec(), b"dst/".to_vec()));
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

    assert!(
        filtered.contains(r#"M 100644 :1 "dst/quo\"te\\caf\303\251.txt""#),
        "expected modify line with escaped quote/backslash/octal bytes:\n{}",
        filtered
    );
    assert!(
        filtered.contains(r#"C "dst/quo\"te\\caf\303\251.txt" "dst/dup\"te\\caf\303\251.txt""#),
        "expected copy line to preserve escaping and rename:\n{}",
        filtered
    );
    assert!(
        filtered.contains(r#"R "dst/quo\"te\\caf\303\251.txt" "dst/fin\"al\\caf\303\251.txt""#),
        "expected rename line to preserve escaping and rename:\n{}",
        filtered
    );
    assert!(
        filtered.contains(r#"D "dst/dup\"te\\caf\303\251.txt""#),
        "expected delete line to preserve escaping and rename:\n{}",
        filtered
    );
    assert!(
        !filtered.contains(r#""src/quo\"te\\caf\303\251.txt""#),
        "expected source prefix fully renamed:\n{}",
        filtered
    );
}

#[test]
fn quoted_modify_path_with_crlf_is_still_parsed_and_renamed() {
    let repo = init_repo();
    let stream_path = repo.join("fe-renames-crlf.stream");
    let stream = "blob\n\
mark :1\n\
data 4\n\
one\n\
\n\
commit refs/heads/main\n\
mark :2\n\
author Tester <tester@example.com> 0 +0000\n\
committer Tester <tester@example.com> 0 +0000\n\
data 3\n\
c1\n\
M 100644 :1 \"src/sp ace.txt\"\r\n\
\n\
done\n";
    std::fs::write(&stream_path, stream).expect("write crlf fast-export stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.path_renames.push((b"src/".to_vec(), b"dst/".to_vec()));
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

    assert!(
        filtered.contains(r#"M 100644 :1 "dst/sp ace.txt""#),
        "expected CRLF-terminated quoted path to be parsed and renamed:\n{}",
        filtered
    );
    assert!(
        !filtered.contains(r#"M 100644 :1 "src/sp ace.txt""#),
        "expected original prefix to be removed after rename:\n{}",
        filtered
    );
}

#[test]
fn path_compat_policy_sanitize_rewrites_on_windows_or_noops_elsewhere() {
    let repo = init_repo();
    let stream_path = repo.join("fe-path-compat-sanitize.stream");
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
    std::fs::write(&stream_path, stream).expect("write path-compat sanitize stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Sanitize;
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
    if cfg!(windows) {
        assert!(
            filtered.contains("M 100644 :1 bad_name_.txt"),
            "expected sanitized path in filtered stream:\n{}",
            filtered
        );
    } else {
        assert!(
            filtered.contains("bad:name?.txt "),
            "expected non-windows host to keep original path:\n{}",
            filtered
        );
    }
}

#[test]
fn path_compat_policy_skip_drops_on_windows_or_noops_elsewhere() {
    let repo = init_repo();
    let stream_path = repo.join("fe-path-compat-skip.stream");
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
    std::fs::write(&stream_path, stream).expect("write path-compat skip stream");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Skip;
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
    if cfg!(windows) {
        assert!(
            !filtered.contains("bad:name?.txt") && !filtered.contains("bad_name_.txt"),
            "expected incompatible path filechange to be dropped:\n{}",
            filtered
        );
    } else {
        assert!(
            filtered.contains("bad:name?.txt "),
            "expected non-windows host to keep original path:\n{}",
            filtered
        );
    }
}

#[test]
fn path_compat_policy_error_fails_on_windows_or_noops_elsewhere() {
    let repo = init_repo();
    let stream_path = repo.join("fe-path-compat-error.stream");
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
    std::fs::write(&stream_path, stream).expect("write path-compat error stream");

    if cfg!(windows) {
        let err = run_tool(&repo, |o| {
            o.debug_mode = true;
            o.dry_run = true;
            o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Error;
            #[allow(deprecated)]
            {
                o.fe_stream_override = Some(stream_path.clone());
            }
        })
        .expect_err("error policy should fail on first incompatible path");

        let msg = err.to_string();
        assert!(
            msg.contains("--path-compat-policy=error"),
            "expected policy context in error: {msg}"
        );
    } else {
        run_tool_expect_success(&repo, |o| {
            o.debug_mode = true;
            o.dry_run = true;
            o.path_compat_policy = filter_repo_rs::pathutil::PathCompatPolicy::Error;
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
        assert!(
            filtered.contains("bad:name?.txt "),
            "expected non-windows host to keep original path:\n{}",
            filtered
        );
    }
}

#[test]
fn inline_replace_text_and_report_modified() {
    let repo = init_repo();
    let stream_path = repo.join("fe-inline.stream");
    let payload = "token=SECRET-INLINE-123\n";
    let payload_len = payload.len();
    let msg = "inline commit\n";
    let msg_len = msg.len();
    let mut s = String::new();
    let (_hc, headref, _he) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let commit_ref = headref.trim();
    s.push_str(&format!("commit {}\n", commit_ref));
    s.push_str("mark :1\n");
    s.push_str("committer A U Thor <a.u.thor@example.com> 1737070000 +0000\n");
    s.push_str(&format!("data {}\n{}", msg_len, msg));
    s.push_str("M 100644 inline secret.txt\n");
    s.push_str(&format!("data {}\n{}", payload_len, payload));
    s.push('\n');
    s.push_str("done\n");
    std::fs::write(&stream_path, s).unwrap();

    let repl = repo.join("repl-inline.txt");
    std::fs::write(&repl, "SECRET-INLINE-123==>REDACTED\n").unwrap();

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
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

#[test]
fn streaming_replace_text_without_match_reports_zero_modified_blobs() {
    let repo = init_repo();
    let stream_path = repo.join("fe-large-blob.stream");
    let payload = vec![b'x'; 1_100_000];
    let msg = b"streaming no-match\n";
    let (_code, headref, _stderr) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let commit_ref = headref.trim();

    let mut stream = Vec::new();
    stream.extend_from_slice(b"blob\nmark :1\n");
    stream.extend_from_slice(format!("data {}\n", payload.len()).as_bytes());
    stream.extend_from_slice(&payload);
    stream.extend_from_slice(b"\n\n");
    stream.extend_from_slice(format!("commit {}\n", commit_ref).as_bytes());
    stream.extend_from_slice(b"mark :2\n");
    stream.extend_from_slice(b"committer A U Thor <a.u.thor@example.com> 1737070001 +0000\n");
    stream.extend_from_slice(format!("data {}\n", msg.len()).as_bytes());
    stream.extend_from_slice(msg);
    stream.extend_from_slice(b"M 100644 :1 large.bin\n\n");
    stream.extend_from_slice(b"done\n");
    std::fs::write(&stream_path, stream).expect("write large blob stream");

    let repl = repo.join("repl-no-match.txt");
    std::fs::write(
        &repl,
        "SECRET-1==>REDACTED\nSECRET-2==>REDACTED\nSECRET-3==>REDACTED\n",
    )
    .expect("write replacement rules");

    run_tool_expect_success(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        o.write_report = true;
        o.replace_text_file = Some(repl.clone());
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    });

    let report = repo.join(".git").join("filter-repo").join("report.txt");
    let mut report_text = String::new();
    std::fs::File::open(&report)
        .expect("open report")
        .read_to_string(&mut report_text)
        .expect("read report");
    assert!(
        report_text.contains("Blobs modified by replace-text: 0"),
        "unexpected modified blob count in report:\n{report_text}"
    );
}

#[test]
fn fe_stream_override_requires_debug_mode() {
    let repo = init_repo();
    let stream_path = repo.join("override.stream");
    std::fs::write(
        &stream_path,
        "blob\nmark :1\ndata 0\n\ncommit refs/heads/main\nmark :2\ndata 0\ndone\n",
    )
    .expect("write dummy stream");

    let err = run_tool(&repo, |o| {
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    })
    .expect_err("fe_stream_override without debug should error");

    let msg = format!("{}", err);
    assert!(
        msg.contains("FRRS_DEBUG"),
        "gating error should mention FRRS_DEBUG"
    );
}

#[test]
fn tag_data_block_rejects_oversized_header_without_panicking() {
    let repo = init_repo();
    let stream_path = repo.join("oversized-tag.stream");
    let stream = format!("tag v1\nfrom :1\ndata {}\n", usize::MAX);
    std::fs::write(&stream_path, stream).expect("write oversized tag stream");

    let err = run_tool(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    })
    .expect_err("oversized tag data should return an error");

    let msg = format!("{err}");
    assert!(
        msg.contains("exceeds maximum allowed size"),
        "unexpected error: {msg}"
    );
}

#[test]
fn commit_message_data_rejects_oversized_header_without_panicking() {
    let repo = init_repo();
    let (_code, headref, _stderr) = run_git(&repo, &["symbolic-ref", "-q", "HEAD"]);
    let commit_ref = headref.trim();
    let stream_path = repo.join("oversized-commit.stream");
    let stream = format!(
        "commit {}\nmark :1\ncommitter Tester <tester@example.com> 0 +0000\ndata {}\n",
        commit_ref,
        usize::MAX
    );
    std::fs::write(&stream_path, stream).expect("write oversized commit stream");

    let err = run_tool(&repo, |o| {
        o.debug_mode = true;
        o.dry_run = true;
        #[allow(deprecated)]
        {
            o.fe_stream_override = Some(stream_path.clone());
        }
    })
    .expect_err("oversized commit data should return an error");

    let msg = format!("{err}");
    assert!(
        msg.contains("exceeds maximum allowed size"),
        "unexpected error: {msg}"
    );
}
