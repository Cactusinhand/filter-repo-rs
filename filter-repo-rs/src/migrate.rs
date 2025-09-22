use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::git_config::GitConfig;
use crate::gitutil;
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
    let refs = match gitutil::get_all_refs(&opts.source) {
        Ok(refs) => refs,
        Err(_) => return Ok(()),
    };
    let mut to_create: Vec<(String, String)> = Vec::new();
    let mut to_delete: Vec<(String, String)> = Vec::new();
    for (refname, hash) in refs
        .iter()
        .filter(|(name, _)| name.starts_with("refs/remotes/origin/"))
    {
        let hash = hash.clone();
        if refname == "refs/remotes/origin/HEAD" {
            to_delete.push((refname.clone(), hash));
            continue;
        }
        let suffix = refname
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(&refname);
        let newref = format!("refs/heads/{}", suffix);
        // Only create if newref does not exist
        let exist = refs.contains_key(&newref);
        if !exist {
            to_create.push((newref, hash.clone()));
        }
        to_delete.push((refname.clone(), hash));
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
    let url = GitConfig::get_string_config(&opts.target, "remote.origin.url")
        .ok()
        .and_then(|value| value)
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
