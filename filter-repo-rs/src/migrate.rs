use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::opts::Options;

#[allow(dead_code)]
pub fn fetch_all_refs_if_needed(opts: &Options) {
    if !opts.sensitive || opts.no_fetch || opts.dry_run {
        return;
    }
    // Check that origin exists
    let remotes = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("remote")
        .output();
    if let Ok(out) = remotes {
        if !out.status.success() {
            return;
        }
        let r = String::from_utf8_lossy(&out.stdout);
        if !r.lines().any(|l| l.trim() == "origin") {
            return;
        }
    } else {
        return;
    }
    // Fetch all refs to ensure sensitive-history coverage
    eprintln!("NOTICE: Fetching all refs from origin to ensure full sensitive-history coverage");
    let _ = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("fetch")
        .arg("-q")
        .arg("--prune")
        .arg("--update-head-ok")
        .arg("--refmap")
        .arg("")
        .arg("origin")
        .arg("+refs/*:refs/*")
        .status();
}

#[allow(dead_code)]
pub fn migrate_origin_to_heads(opts: &Options) -> io::Result<()> {
    if opts.partial || opts.dry_run {
        return Ok(());
    }
    // List refs under refs/remotes/origin/*
    let out = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("for-each-ref")
        .arg("--format=%(refname) %(objectname)")
        .arg("refs/remotes/origin")
        .output()
        .ok();
    let out = match out {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Ok(()),
    };
    let mut to_create: Vec<(String, String)> = Vec::new();
    let mut to_delete: Vec<(String, String)> = Vec::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let r = match (it.next(), it.next()) {
            (Some(r), Some(h)) => (r.to_string(), h.to_string()),
            _ => continue,
        };
        let (refname, hash) = r;
        if refname == "refs/remotes/origin/HEAD" {
            to_delete.push((refname, hash));
            continue;
        }
        let suffix = refname
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(&refname);
        let newref = format!("refs/heads/{}", suffix);
        // Only create if newref does not exist
        let exist = Command::new("git")
            .arg("-C")
            .arg(&opts.source)
            .arg("show-ref")
            .arg("--verify")
            .arg(&newref)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()
            .map(|s| s.success())
            .unwrap_or(false);
        if !exist {
            to_create.push((newref, hash.clone()));
        }
        to_delete.push((refname, hash));
    }
    if to_create.is_empty() && to_delete.is_empty() {
        return Ok(());
    }
    // Batch update-ref
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&opts.source)
        .arg("update-ref")
        .arg("--no-deref")
        .arg("--stdin")
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        for (r, h) in to_create.iter() {
            let _ = writeln!(stdin, "create {} {}", r, h);
        }
        for (r, h) in to_delete.iter() {
            let _ = writeln!(stdin, "delete {} {}", r, h);
        }
    }
    let _ = child.wait();
    Ok(())
}

pub fn remove_origin_remote_if_applicable(opts: &Options) {
    if opts.sensitive || opts.partial || opts.dry_run {
        return;
    }
    // Check that origin exists
    let remotes = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("remote")
        .output();
    if let Ok(out) = remotes {
        if !out.status.success() {
            return;
        }
        let r = String::from_utf8_lossy(&out.stdout);
        if !r.lines().any(|l| l.trim() == "origin") {
            return;
        }
    } else {
        return;
    }
    // Print URL for context if available
    let url = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("config")
        .arg("--get")
        .arg("remote.origin.url")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    if url.is_empty() {
        eprintln!("NOTICE: Removing 'origin' remote; see docs if you want to push back there.");
    } else {
        eprintln!("NOTICE: Removing 'origin' remote (was: {})", url);
    }
    let _ = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("remote")
        .arg("rm")
        .arg("origin")
        .status();
}
