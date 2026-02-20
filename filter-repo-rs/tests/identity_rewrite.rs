mod common;
use common::*;

use std::process::Command;

fn commit_with_identity(
    repo: &std::path::Path,
    rel_path: &str,
    contents: &str,
    message: &str,
    author_name: &str,
    author_email: &str,
    committer_name: &str,
    committer_email: &str,
) {
    write_file(repo, rel_path, contents);
    assert_eq!(
        run_git(repo, &["add", rel_path]).0,
        0,
        "git add should succeed"
    );

    let output = Command::new("git")
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", author_name)
        .env("GIT_AUTHOR_EMAIL", author_email)
        .env("GIT_COMMITTER_NAME", committer_name)
        .env("GIT_COMMITTER_EMAIL", committer_email)
        .arg("commit")
        .arg("-m")
        .arg(message)
        .output()
        .expect("run git commit with custom identity");

    assert!(
        output.status.success(),
        "commit failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn mailmap_rewrites_author_and_committer_identities() {
    let repo = init_repo();
    commit_with_identity(
        &repo,
        "mailmap-target.txt",
        "payload",
        "commit with legacy identity",
        "Old Author",
        "old@example.com",
        "Old Committer",
        "old@example.com",
    );

    let mailmap = repo.join("rewrite.mailmap");
    std::fs::write(
        &mailmap,
        "Canonical Name <canonical@example.com> <old@example.com>\n",
    )
    .expect("write mailmap rules");

    run_tool_expect_success(&repo, |o| {
        o.mailmap_file = Some(mailmap.clone());
        o.no_data = true;
    });

    let (_code, identity, _stderr) = run_git(&repo, &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]);
    assert_eq!(
        identity.trim(),
        "Canonical Name <canonical@example.com>|Canonical Name <canonical@example.com>"
    );
}

#[test]
fn mailmap_takes_precedence_over_other_identity_rewriters() {
    let repo = init_repo();
    commit_with_identity(
        &repo,
        "precedence-target.txt",
        "payload",
        "commit for precedence check",
        "Old Author",
        "old@example.com",
        "Old Committer",
        "old@example.com",
    );

    let mailmap = repo.join("rewrite.mailmap");
    let author_rules = repo.join("author-rules.txt");
    let committer_rules = repo.join("committer-rules.txt");
    let email_rules = repo.join("email-rules.txt");

    std::fs::write(
        &mailmap,
        "Mailmap Name <mailmap@example.com> <old@example.com>\n",
    )
    .expect("write mailmap rules");
    std::fs::write(
        &author_rules,
        "Old Author==>Author Rule Name\nold@example.com==>author-rule@example.com\n",
    )
    .expect("write author rules");
    std::fs::write(
        &committer_rules,
        "Old Committer==>Committer Rule Name\nold@example.com==>committer-rule@example.com\n",
    )
    .expect("write committer rules");
    std::fs::write(&email_rules, "old@example.com==>email-rule@example.com\n")
        .expect("write email rules");

    run_tool_expect_success(&repo, |o| {
        o.mailmap_file = Some(mailmap.clone());
        o.author_rewrite_file = Some(author_rules.clone());
        o.committer_rewrite_file = Some(committer_rules.clone());
        o.email_rewrite_file = Some(email_rules.clone());
        o.no_data = true;
    });

    let (_code, identity, _stderr) = run_git(&repo, &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]);
    let identity = identity.trim();
    assert_eq!(
        identity,
        "Mailmap Name <mailmap@example.com>|Mailmap Name <mailmap@example.com>"
    );
    assert!(
        !identity.contains("Rule Name") && !identity.contains("email-rule@example.com"),
        "mailmap should take precedence over other identity rewriters"
    );
}
