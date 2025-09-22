use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::migrate;
use crate::opts::Options;
use crate::stream::BlobSizeTracker;

#[derive(Debug)]
pub struct ReportData {
    pub stripped_by_size: usize,
    pub stripped_by_sha: usize,
    pub modified_blobs: usize,
    pub samples_size: Vec<Vec<u8>>,     // paths
    pub samples_sha: Vec<Vec<u8>>,      // paths
    pub samples_modified: Vec<Vec<u8>>, // paths
}

// Flush buffered lightweight tag resets to outputs prior to sending 'done'.
pub fn flush_lightweight_tag_resets(
    buffered_tag_resets: &mut Vec<(Vec<u8>, Vec<u8>)>,
    annotated_tag_refs: &BTreeSet<Vec<u8>>,
    filt_file: &mut File,
    mut fi_in: Option<&mut ChildStdin>,
    import_broken: &mut bool,
) -> io::Result<()> {
    if buffered_tag_resets.is_empty() {
        return Ok(());
    }
    let mut emitted: BTreeSet<Vec<u8>> = BTreeSet::new();
    let items = std::mem::take(buffered_tag_resets);
    for (ref_full, from_line) in items.into_iter() {
        if annotated_tag_refs.contains(&ref_full) {
            continue;
        }
        if emitted.contains(&ref_full) {
            continue;
        }
        let mut reset_line = Vec::with_capacity(7 + ref_full.len() + 1);
        reset_line.extend_from_slice(b"reset ");
        reset_line.extend_from_slice(&ref_full);
        reset_line.push(b'\n');
        filt_file.write_all(&reset_line)?;
        filt_file.write_all(&from_line)?;
        if let Some(ref mut fi) = fi_in {
            if let Err(e) = fi.write_all(&reset_line) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *import_broken = true;
                } else {
                    return Err(e);
                }
            }
            if let Err(e) = fi.write_all(&from_line) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *import_broken = true;
                } else {
                    return Err(e);
                }
            }
        }

        emitted.insert(ref_full);
    }
    Ok(())
}

pub fn finalize(
    opts: &Options,
    debug_dir: &Path,
    ref_renames: BTreeSet<(Vec<u8>, Vec<u8>)>,
    commit_pairs: Vec<(Vec<u8>, Option<u32>)>,
    buffered_tag_resets: Vec<(Vec<u8>, Vec<u8>)>,
    annotated_tag_refs: BTreeSet<Vec<u8>>,
    updated_branch_refs: BTreeSet<Vec<u8>>,
    mut branch_reset_targets: Vec<(Vec<u8>, Vec<u8>)>,
    filt_file: &mut File,
    mut fi_in: Option<ChildStdin>,
    fe: &mut Child,
    fi: Option<&mut Child>,
    mut import_broken: bool,
    allow_flush_tag_resets: bool,
    report: Option<ReportData>,
    blob_sizes: &BlobSizeTracker,
) -> io::Result<()> {
    // Emit buffered lightweight tag resets if any remain (ideally flushed before 'done')
    if allow_flush_tag_resets {
        let mut buffered = buffered_tag_resets;
        if !buffered.is_empty() {
            flush_lightweight_tag_resets(
                &mut buffered,
                &annotated_tag_refs,
                filt_file,
                fi_in.as_mut(),
                &mut import_broken,
            )?;
        }
    }
    if let Some(stdin) = fi_in.take() {
        drop(stdin);
    }

    // Handle process termination and propagate errors
    if import_broken {
        let _ = fe.kill();
    }
    let fe_status = fe.wait()?;
    if !fe_status.success() {
        eprintln!("fast-export failed: {}", fe_status);
        std::process::exit(fe_status.code().unwrap_or(1));
    }
    if let Some(child) = fi {
        let fi_status = child.wait()?;
        if !fi_status.success() {
            eprintln!("fast-import failed: {}", fi_status);
            std::process::exit(fi_status.code().unwrap_or(1));
        }
    }

    let refs: Vec<(Vec<u8>, Vec<u8>)> = ref_renames.into_iter().collect();
    if !refs.is_empty() {
        let mut f = File::create(debug_dir.join("ref-map"))?;
        for (old, new_) in &refs {
            f.write_all(&old)?;
            f.write_all(b" ")?;
            f.write_all(&new_)?;
            f.write_all(b"\n")?;
        }
    }

    // Load exported marks so we can resolve mark references to object ids
    let marks_path = debug_dir.join("target-marks");
    let mut mark_to_id: HashMap<u32, Vec<u8>> = HashMap::new();
    if let Ok(marks) = File::open(&marks_path) {
        let mut rdr = BufReader::new(marks);
        let mut buf = String::new();
        while rdr.read_line(&mut buf).unwrap_or(0) > 0 {
            let line = buf.trim_end();
            let mut it = line.split_whitespace();
            if let (Some(mark_s), Some(id_s)) = (it.next(), it.next()) {
                if let Some(mark_num) = mark_s.strip_prefix(":").and_then(|s| s.parse::<u32>().ok())
                {
                    mark_to_id.insert(mark_num, id_s.as_bytes().to_vec());
                }
            }
            buf.clear();
        }
    }

    if !opts.dry_run {
        let mut resolved_updates: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        for (refname, target) in branch_reset_targets.drain(..) {
            if let Some(oid) = resolve_reset_target(&target, &mark_to_id, opts)? {
                resolved_updates.insert(refname, oid);
            }
        }
        let mut update_payload: Vec<u8> = Vec::new();
        for (refname, oid) in &resolved_updates {
            let ref_str = String::from_utf8_lossy(refname);
            let oid_str = String::from_utf8_lossy(oid);
            update_payload
                .extend_from_slice(format!("update {} {}\n", ref_str, oid_str).as_bytes());
        }
        for (old, new_) in &refs {
            if old == new_ {
                continue;
            }
            let old_ref = String::from_utf8_lossy(old).to_string();
            let resolve = Command::new("git")
                .arg("-C")
                .arg(&opts.target)
                .arg("for-each-ref")
                .arg("--format=%(refname)")
                .arg(&old_ref)
                .output();
            let mut delete_old = false;
            let mut resolved_name: Option<Vec<u8>> = None;
            match resolve {
                Ok(output) => {
                    if output.status.success() {
                        resolved_name = output
                            .stdout
                            .split(|b| *b == b'\n')
                            .filter_map(|line| {
                                let mut trimmed = line;
                                if let Some(b'\r') = trimmed.last() {
                                    trimmed = &trimmed[..trimmed.len() - 1];
                                }
                                if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_vec())
                                }
                            })
                            .next();
                        if let Some(refname) = &resolved_name {
                            if refname.as_slice() == old.as_slice() {
                                delete_old = true;
                            }
                        }
                    } else {
                        eprintln!(
                            "warning: failed to query existing ref {}: {}",
                            old_ref, output.status
                        );
                    }
                }
                Err(err) => {
                    eprintln!("warning: failed to query existing ref {}: {}", old_ref, err);
                }
            }
            if delete_old {
                update_payload.extend_from_slice(b"delete ");
                update_payload.extend_from_slice(old);
                update_payload.push(b'\n');
            } else if let Some(refname) = resolved_name {
                eprintln!(
                    "warning: not deleting {} because repository resolves to {}",
                    old_ref,
                    String::from_utf8_lossy(&refname),
                );
            } else {
                eprintln!(
                    "warning: not deleting {} because it does not exist",
                    old_ref,
                );
            }
        }
        if !update_payload.is_empty() {
            let mut child = Command::new("git")
                .arg("-C")
                .arg(&opts.target)
                .arg("update-ref")
                .arg("--no-deref")
                .arg("--stdin")
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("failed to run git update-ref: {e}"),
                    )
                })?;
            if let Some(mut sin) = child.stdin.take() {
                sin.write_all(&update_payload)?;
            }
            let status = child.wait()?;
            if !status.success() {
                eprintln!("warning: git update-ref operations failed: {}", status);
            }
        }
    }

    // Write commit-map (old -> new) using exported marks. If in-memory pairs empty,
    // fall back to scanning the filtered stream for commit mark/original-oid pairs.
    let mut pairs = commit_pairs;
    if pairs.is_empty() {
        let filtered = debug_dir.join("fast-export.filtered");
        if let Ok(fh) = File::open(&filtered) {
            let mut rdr = BufReader::new(fh);
            let mut line = Vec::with_capacity(256);
            let mut in_commit = false;
            let mut cur_mark: Option<u32> = None;
            let mut cur_old: Option<Vec<u8>> = None;
            loop {
                line.clear();
                let n = rdr.read_until(b'\n', &mut line)?;
                if n == 0 {
                    break;
                }
                if line.starts_with(b"commit ") {
                    in_commit = true;
                    cur_mark = None;
                    cur_old = None;
                    continue;
                }
                if !in_commit {
                    continue;
                }
                if line.starts_with(b"mark :") {
                    // parse mark
                    let mut num: u32 = 0;
                    let mut seen = false;
                    for &b in line[b"mark :".len()..].iter() {
                        if b >= b'0' && b <= b'9' {
                            seen = true;
                            num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
                        } else {
                            break;
                        }
                    }
                    if seen {
                        cur_mark = Some(num);
                    }
                    continue;
                }
                if line.starts_with(b"original-oid ") {
                    let mut v = line[b"original-oid ".len()..].to_vec();
                    if let Some(last) = v.last() {
                        if *last == b'\n' {
                            v.pop();
                        }
                    }
                    cur_old = Some(v);
                    continue;
                }
                if line.starts_with(b"data ") {
                    // skip payload
                    let size_bytes = &line[b"data ".len()..];
                    let n: usize = std::str::from_utf8(size_bytes)
                        .ok()
                        .map(|s| s.trim())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let mut buf = vec![0u8; n];
                    rdr.read_exact(&mut buf)?;
                    continue;
                }
                if line == b"\n" {
                    if let (Some(m), Some(old)) = (cur_mark.take(), cur_old.take()) {
                        pairs.push((old, Some(m)));
                    }
                    in_commit = false;
                    continue;
                }
            }
        }
    }

    // Always create commit-map (even if empty) for user tooling parity
    {
        let mut f = File::create(debug_dir.join("commit-map"))?;
        for (old, mark) in pairs {
            match mark {
                Some(m) => {
                    if let Some(newid) = mark_to_id.get(&m) {
                        f.write_all(&old)?;
                        f.write_all(b" ")?;
                        f.write_all(newid)?;
                        f.write_all(b"\n")?;
                    }
                }
                None => {
                    f.write_all(&old)?;
                    f.write_all(b" 0000000000000000000000000000000000000000\n")?;
                }
            }
        }
    }

    // Optional reset --hard on target
    if !opts.dry_run && opts.reset {
        let mut reset = Command::new("git");
        reset.arg("-C").arg(&opts.target).arg("reset");
        if opts.quiet {
            reset.arg("--quiet");
        }
        reset.arg("--hard");
        let status = reset.status()?;
        if !status.success() {
            eprintln!("warning: 'git reset --hard' failed: {}", status);
        }
    }

    // Optional post-import cleanup
    if !opts.dry_run {
        match opts.cleanup {
            crate::opts::CleanupMode::None => {}
            crate::opts::CleanupMode::Standard => {
                run_repo_cleanup(&opts.target, false);
            }
            crate::opts::CleanupMode::Aggressive => {
                run_repo_cleanup(&opts.target, true);
            }
        }
    }

    // Optional reporting
    if opts.write_report {
        // Ensure debug filtered stream is flushed before scanning
        let _ = filt_file.flush();
        let mut f = File::create(debug_dir.join("report.txt"))?;
        if let Some(r) = report {
            // Augment sampling: when max-blob-size is set, scan streams for dropped paths and oversize refs
            let mut size_samples = r.samples_size;
            if opts.max_blob_size.is_some() {
                // First try scanning the filtered stream for dropped paths (D <path>)
                let filtered = debug_dir.join("fast-export.filtered");
                if let Ok(fh) = File::open(&filtered) {
                    let mut rdr = BufReader::new(fh);
                    let mut line = Vec::with_capacity(256);
                    while rdr.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                        if line.starts_with(b"D ") {
                            let mut p = line[2..].to_vec();
                            if let Some(last) = p.last() {
                                if *last == b'\n' {
                                    p.pop();
                                }
                            }
                            if let Some(last) = p.last() {
                                if *last == b'\r' {
                                    p.pop();
                                }
                            }
                            if !p.is_empty() && !size_samples.iter().any(|e| e == &p) {
                                size_samples.push(p);
                            }
                            if size_samples.len() >= 20 {
                                break;
                            }
                        }
                        line.clear();
                    }
                }
                // If still under limit, scan original stream: map oversize blob marks to commit M-lines, and oversize SHAs
                if size_samples.len() < 20 {
                    let original = debug_dir.join("fast-export.original");
                    if let Ok(fh) = File::open(&original) {
                        let mut rdr = BufReader::new(fh);
                        let mut line = Vec::with_capacity(256);
                        let mut in_blob = false;
                        let mut last_mark: Option<u32> = None;
                        let mut oversize_marks: std::collections::HashSet<u32> =
                            std::collections::HashSet::new();
                        // Pass 1: collect oversize marks
                        loop {
                            line.clear();
                            if rdr.read_until(b'\n', &mut line).unwrap_or(0) == 0 {
                                break;
                            }
                            if line == b"blob\n" {
                                in_blob = true;
                                last_mark = None;
                                continue;
                            }
                            if in_blob && line.starts_with(b"mark :") {
                                let mut num: u32 = 0;
                                let mut seen = false;
                                for &b in line[b"mark :".len()..].iter() {
                                    if b >= b'0' && b <= b'9' {
                                        seen = true;
                                        num = num
                                            .saturating_mul(10)
                                            .saturating_add((b - b'0') as u32);
                                    } else {
                                        break;
                                    }
                                }
                                if seen {
                                    last_mark = Some(num);
                                }
                                continue;
                            }
                            if in_blob && line.starts_with(b"data ") {
                                let size_bytes = &line[b"data ".len()..];
                                let n = std::str::from_utf8(size_bytes)
                                    .ok()
                                    .map(|s| s.trim())
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .unwrap_or(0);
                                // consume payload
                                let mut buf = vec![0u8; n];
                                let _ = rdr.read_exact(&mut buf);
                                if let (Some(max), Some(m)) = (opts.max_blob_size, last_mark) {
                                    if n > max {
                                        oversize_marks.insert(m);
                                    }
                                }
                                in_blob = false;
                                last_mark = None;
                                continue;
                            }
                            if in_blob && line == b"\n" {
                                in_blob = false;
                                last_mark = None;
                                continue;
                            }
                        }
                        // Pass 2: find commit M-lines referencing oversize marks and collect paths
                        let mut rdr2 = BufReader::new(File::open(&original)?);
                        let mut line2 = Vec::with_capacity(256);
                        loop {
                            line2.clear();
                            if rdr2.read_until(b'\n', &mut line2).unwrap_or(0) == 0 {
                                break;
                            }
                            if line2.starts_with(b"M ") {
                                // parse id and path
                                let bytes = &line2;
                                let mut i = 2;
                                while i < bytes.len() && bytes[i] != b' ' {
                                    i += 1;
                                }
                                if i < bytes.len() {
                                    i += 1;
                                }
                                let id_start = i;
                                while i < bytes.len() && bytes[i] != b' ' {
                                    i += 1;
                                }
                                let id_end = i;
                                let path_start = if i < bytes.len() { i + 1 } else { bytes.len() };
                                let id = &bytes[id_start..id_end];
                                if id.first().copied() == Some(b':') {
                                    let mut num: u32 = 0;
                                    let mut seen = false;
                                    let mut j = 1;
                                    while j < id.len() {
                                        let b = id[j];
                                        if b >= b'0' && b <= b'9' {
                                            seen = true;
                                            num = num
                                                .saturating_mul(10)
                                                .saturating_add((b - b'0') as u32);
                                        } else {
                                            break;
                                        }
                                        j += 1;
                                    }
                                    if seen && oversize_marks.contains(&num) {
                                        let mut p = bytes[path_start..].to_vec();
                                        if let Some(last) = p.last() {
                                            if *last == b'\n' {
                                                p.pop();
                                            }
                                        }
                                        if !p.is_empty() && !size_samples.iter().any(|e| e == &p) {
                                            size_samples.push(p);
                                        }
                                        if size_samples.len() >= 20 {
                                            break;
                                        }
                                    }
                                } else if id.len() == 40
                                    && id.iter().all(|b| {
                                        (*b >= b'0' && *b <= b'9') || (*b >= b'a' && *b <= b'f')
                                    })
                                {
                                    // SHA1: use pre-computed oversize tracker
                                    let sha = id;
                                    if blob_sizes.known_oversize(sha) {
                                        let mut p = bytes[path_start..].to_vec();
                                        if let Some(last) = p.last() {
                                            if *last == b'\n' {
                                                p.pop();
                                            }
                                        }
                                        if !p.is_empty() && !size_samples.iter().any(|e| e == &p) {
                                            size_samples.push(p);
                                        }
                                        if size_samples.len() >= 20 {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let size_count = std::cmp::max(r.stripped_by_size, size_samples.len());
            writeln!(f, "Blobs stripped by size: {}", size_count)?;
            writeln!(f, "Blobs stripped by SHA: {}", r.stripped_by_sha)?;
            writeln!(f, "Blobs modified by replace-text: {}", r.modified_blobs)?;
            if !size_samples.is_empty() {
                writeln!(f, "\nSample paths (size):")?;
                for p in size_samples {
                    f.write_all(&p)?;
                    f.write_all(b"\n")?;
                }
            }
            if !r.samples_sha.is_empty() {
                writeln!(f, "\nSample paths (sha):")?;
                for p in r.samples_sha {
                    f.write_all(&p)?;
                    f.write_all(b"\n")?;
                }
            }
            if !r.samples_modified.is_empty() {
                writeln!(f, "\nSample paths (modified):")?;
                for p in r.samples_modified {
                    f.write_all(&p)?;
                    f.write_all(b"\n")?;
                }
            }
        } else {
            writeln!(f, "No report data collected.")?;
        }
    }

    // Finalize HEAD: if HEAD points to a non-existent branch, try to remap;
    // if detached or missing, prefer first updated branch or first existing branch.
    // Get HEAD symbolic ref (if any)
    let head_ref = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("symbolic-ref")
        .arg("-q")
        .arg("HEAD")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !opts.dry_run {
        if head_ref.status.success() {
            let head = String::from_utf8_lossy(&head_ref.stdout).trim().to_string();
            // If current HEAD target exists, keep as-is
            let ok = Command::new("git")
                .arg("-C")
                .arg(&opts.target)
                .arg("show-ref")
                .arg("--verify")
                .arg(&head)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?
                .success();
            if !ok {
                // Try to map HEAD using branch_rename if applicable
                let mut updated_head: Option<String> = None;
                if let Some((ref old, ref new_)) = opts.branch_rename {
                    if let Some(tail) = head.strip_prefix("refs/heads/") {
                        let tail_b = tail.as_bytes();
                        if tail_b.starts_with(&old[..]) {
                            let mut new_full = Vec::with_capacity(
                                "refs/heads/".len()
                                    + new_.len()
                                    + (tail_b.len().saturating_sub(old.len())),
                            );
                            new_full.extend_from_slice(b"refs/heads/");
                            new_full.extend_from_slice(&new_);
                            new_full.extend_from_slice(&tail_b[old.len()..]);
                            let new_str = String::from_utf8_lossy(&new_full).to_string();
                            let exists = Command::new("git")
                                .arg("-C")
                                .arg(&opts.target)
                                .arg("show-ref")
                                .arg("--verify")
                                .arg(&new_str)
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status()?
                                .success();
                            if exists {
                                updated_head = Some(new_str);
                            }
                        }
                    }
                }
                // Choose a suitable branch: updated branch, or first existing branch
                let fallback = updated_head
                    .or_else(|| {
                        updated_branch_refs
                            .iter()
                            .next()
                            .map(|b| String::from_utf8_lossy(b).to_string())
                    })
                    .or_else(|| {
                        let out = Command::new("git")
                            .arg("-C")
                            .arg(&opts.target)
                            .arg("for-each-ref")
                            .arg("--count=1")
                            .arg("--format=%(refname)")
                            .arg("refs/heads")
                            .output()
                            .ok()?;
                        if out.status.success() {
                            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
                        } else {
                            None
                        }
                    });
                if let Some(refstr) = fallback.filter(|s| !s.is_empty()) {
                    let status = Command::new("git")
                        .arg("-C")
                        .arg(&opts.target)
                        .arg("symbolic-ref")
                        .arg("HEAD")
                        .arg(&refstr)
                        .status()?;
                    if !status.success() {
                        eprintln!("warning: failed to update HEAD to {}: {}", refstr, status);
                    }
                }
            }
        } else {
            // Detached HEAD: if we updated branches, prefer setting HEAD to one
            if let Some(first) = updated_branch_refs.iter().next() {
                let refstr = String::from_utf8_lossy(first).to_string();
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&opts.target)
                    .arg("symbolic-ref")
                    .arg("HEAD")
                    .arg(&refstr)
                    .status()?;
                if !status.success() {
                    eprintln!("warning: failed to update HEAD to {}: {}", refstr, status);
                }
            }
        }
    }

    if !opts.quiet {
        eprintln!(
            "New history written (prototype Rust pipeline). Debug files in {:?}",
            debug_dir
        );
    }
    // Post-run remote cleanup (non-sensitive parity): remove origin
    migrate::remove_origin_remote_if_applicable(opts);
    Ok(())
}

fn run_repo_cleanup(target: &Path, aggressive: bool) {
    let mut reflog = Command::new("git");
    reflog
        .arg("-C")
        .arg(target)
        .arg("reflog")
        .arg("expire")
        .arg("--expire=now");
    if aggressive {
        reflog.arg("--expire-unreachable=now");
    }
    reflog.arg("--all");
    match reflog.status() {
        Ok(status) if !status.success() => {
            eprintln!("warning: git reflog expire failed: {}", status);
        }
        Err(e) => eprintln!("warning: failed to execute git reflog expire: {}", e),
        _ => {}
    }

    let mut gc = Command::new("git");
    gc.arg("-C")
        .arg(target)
        .arg("gc")
        .arg("--prune=now")
        .arg("--quiet");
    if aggressive {
        gc.arg("--aggressive");
    }
    match gc.status() {
        Ok(status) if !status.success() => {
            eprintln!("warning: git gc failed: {}", status);
        }
        Err(e) => eprintln!("warning: failed to execute git gc: {}", e),
        _ => {}
    }
}

fn resolve_reset_target(
    target: &[u8],
    mark_to_id: &HashMap<u32, Vec<u8>>,
    opts: &Options,
) -> io::Result<Option<Vec<u8>>> {
    if target.is_empty() {
        return Ok(None);
    }
    if target[0] == b':' {
        let mut num: u32 = 0;
        let mut seen = false;
        for &b in &target[1..] {
            if (b'0'..=b'9').contains(&b) {
                seen = true;
                num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
            } else {
                break;
            }
        }
        if seen {
            if let Some(oid) = mark_to_id.get(&num) {
                return Ok(Some(oid.clone()));
            }
            eprintln!(
                "warning: mark :{} not found in target marks; skipping ref update",
                num
            );
            return Ok(None);
        }
    }
    let is_hex = target.len() == 40
        && target
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'));
    if is_hex {
        let mut out = target.to_vec();
        for b in &mut out {
            if (b'A'..=b'F').contains(b) {
                *b = *b + 32;
            }
        }
        return Ok(Some(out));
    }
    let spec = String::from_utf8_lossy(target).to_string();
    if spec.is_empty() {
        return Ok(None);
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(&opts.target)
        .arg("rev-parse")
        .arg("--verify")
        .arg(&spec)
        .output()
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("failed to run git rev-parse: {e}"),
            )
        })?;
    if output.status.success() {
        let oid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if oid.is_empty() {
            return Ok(None);
        }
        return Ok(Some(oid.into_bytes()));
    }
    eprintln!(
        "warning: could not resolve '{}' for ref update: {}",
        spec, output.status,
    );
    Ok(None)
}
