use std::collections::{BTreeSet, HashSet, HashMap};
use std::fs::{create_dir_all, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::Command;


use crate::gitutil::git_dir;
use crate::message::MessageReplacer;
use crate::message::blob_regex::RegexReplacer as BlobRegexReplacer;
use crate::opts::Options;

const REPORT_SAMPLE_LIMIT: usize = 20;

pub fn run(opts: &Options) -> io::Result<()> {
  let target_git_dir = git_dir(&opts.target)
    .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Target {:?} is not a git repo: {e}", opts.target)))?;
  let _ = git_dir(&opts.source)
    .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Source {:?} is not a git repo: {e}", opts.source)))?;

  let debug_dir = target_git_dir.join("filter-repo");
  if !debug_dir.exists() { create_dir_all(&debug_dir)?; }
  let mut orig_file = File::create(debug_dir.join("fast-export.original"))?;
  let mut filt_file = File::create(debug_dir.join("fast-export.filtered"))?;

  let mut fe = crate::pipes::build_fast_export_cmd(opts).spawn().expect("failed to spawn git fast-export");
  let mut fi = if opts.dry_run { None } else { Some(crate::pipes::build_fast_import_cmd(opts).spawn().expect("failed to spawn git fast-import")) };

  let mut fe_out = BufReader::new(fe.stdout.take().expect("no stdout from fast-export"));
  let mut fi_in_opt: Option<std::process::ChildStdin> = if let Some(ref mut child) = fi { child.stdin.take() } else { None };

  let replacer = match &opts.replace_message_file {
    Some(p) => Some(MessageReplacer::from_file(p).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to read --replace-message: {e}")))?),
    None => None,
  };
  let content_replacer = match &opts.replace_text_file {
    Some(p) => Some(MessageReplacer::from_file(p).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to read --replace-text: {e}")))?),
    None => None,
  };
  let content_regex_replacer: Option<BlobRegexReplacer> = match &opts.replace_text_file {
    Some(p) => BlobRegexReplacer::from_file(p).map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to read --replace-text: {e}")))?,
    None => None,
  };

  // minimal stream state is tracked via local booleans and buffers
  // Commit buffering state for pruning
  let mut in_commit = false;
  let mut commit_buf: Vec<u8> = Vec::with_capacity(8192);
  let mut commit_has_changes = false;
  let mut commit_mark: Option<u32> = None;
  let mut first_parent_mark: Option<u32> = None;
  let mut commit_original_oid: Option<Vec<u8>> = None;
  let mut parent_count: usize = 0;
  let mut commit_pairs: Vec<(u32, Vec<u8>)> = Vec::new();
  let mut import_broken = false;
  // If we skip a duplicate annotated tag header, swallow the rest of its block
  let mut skipping_tag_block: bool = false;
  let mut ref_renames: BTreeSet<(Vec<u8>, Vec<u8>)> = BTreeSet::new();
  // Track which refs we have updated (to avoid multiple updates of same ref via tag blocks)
  let mut updated_refs: BTreeSet<Vec<u8>> = BTreeSet::new();
  // Prefer annotated tags: track which tag refs were created by `tag <name>` blocks
  let mut annotated_tag_refs: BTreeSet<Vec<u8>> = BTreeSet::new();
  // Track updated branch refs (refs/heads/*) to help finalize HEAD
  let mut updated_branch_refs: BTreeSet<Vec<u8>> = BTreeSet::new();
  // Buffer lightweight tag resets (ref, from-line)
  let mut buffered_tag_resets: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
  // After seeing a reset refs/tags/<name>, capture the following 'from ...' line
  let mut pending_tag_reset: Option<Vec<u8>> = None;
  // Blob filtering state for --max-blob-size
  let mut in_blob: bool = false;
  let mut blob_buf: Vec<Vec<u8>> = Vec::new();
  let mut last_blob_mark: Option<u32> = None;
  let mut oversize_marks: HashSet<u32> = HashSet::new();
  let mut oversize_shas: HashSet<Vec<u8>> = HashSet::new();
  let strip_sha_set: HashSet<Vec<u8>> = {
    let mut s = HashSet::new();
    if let Some(path) = &opts.strip_blobs_with_ids {
      if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
          let t = line.trim();
          if t.len() == 40 && t.bytes().all(|b| (b'0'..=b'9').contains(&b) || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)) {
            s.insert(t.to_ascii_lowercase().into_bytes());
          }
        }
      }
    }
    s
  };
  let mut last_blob_orig_sha: Option<Vec<u8>> = None;
  let mut sha_size_cache: HashMap<Vec<u8>, usize> = HashMap::new();
  // Reporting accumulators
  let mut suppressed_marks_by_size: HashSet<u32> = HashSet::new();
  let mut suppressed_marks_by_sha: HashSet<u32> = HashSet::new();
  let mut suppressed_shas_by_size: HashSet<Vec<u8>> = HashSet::new();
  let mut suppressed_shas_by_sha: HashSet<Vec<u8>> = HashSet::new();
  let mut modified_marks: HashSet<u32> = HashSet::new();
  let mut samples_size: Vec<Vec<u8>> = Vec::new();
  let mut samples_sha: Vec<Vec<u8>> = Vec::new();
  let mut samples_modified: Vec<Vec<u8>> = Vec::new();
  let mut inline_modified_paths: HashSet<Vec<u8>> = HashSet::new();
  let mut line = Vec::with_capacity(8192);
  // Track if the previous M-line used inline content; store commit_buf position and path bytes
  let mut pending_inline: Option<(usize, Vec<u8>)> = None;
  // Track marks that have been emitted to avoid referencing undeclared marks in aliases
  let mut emitted_marks: HashSet<u32> = HashSet::new();

  loop {
    line.clear();
    let read = fe_out.read_until(b'\n', &mut line)?;
    if read == 0 { break; }

    // Always mirror original header/line
    orig_file.write_all(&line)?;

    // If swallowing a skipped annotated tag block, consume its lines and payload
    if skipping_tag_block {
      if line.starts_with(b"data ") {
        let size_bytes = &line[b"data ".len()..];
        let n = std::str::from_utf8(size_bytes)
          .ok().map(|s| s.trim()).and_then(|s| s.parse::<usize>().ok())
          .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
        let mut payload = vec![0u8; n];
        fe_out.read_exact(&mut payload)?;
        // Mirror original payload to debug file
        orig_file.write_all(&payload)?;
        // Done skipping this tag block
        skipping_tag_block = false;
      }
      continue;
    }

    // Pre-check for duplicate annotated tag: if target ref already updated, swallow this tag block
    if crate::tag::precheck_duplicate_tag(&line, opts, &updated_refs) {
      skipping_tag_block = true;
      continue;
    }

    // In blob header: record and ignore original-oid lines (fast-import does not accept them outside commits/tags)
    if in_blob && line.starts_with(b"original-oid ") {
      let mut v = line[b"original-oid ".len()..].to_vec();
      if let Some(last) = v.last() { if *last == b'\n' { v.pop(); } }
      for b in &mut v { if *b >= b'A' && *b <= b'F' { *b = *b + 32; } }
      last_blob_orig_sha = Some(v);
      continue;
    }

    // Blob begin
    if line == b"blob\n" {
      in_blob = true;
      blob_buf.clear();
      blob_buf.push(line.clone());
      last_blob_mark = None;
      continue;
    }
    // Blob mark
    if in_blob && line.starts_with(b"mark :") {
      let mut num: u32 = 0; let mut seen=false;
      for &b in line[b"mark :".len()..].iter() { if b>=b'0'&&b<=b'9' { seen=true; num=num.saturating_mul(10).saturating_add((b-b'0') as u32);} else {break;} }
      if seen { last_blob_mark = Some(num); }
      blob_buf.push(line.clone());
      continue;
    }

    // If a lightweight tag reset is pending, capture its 'from ' line
    if crate::tag::maybe_capture_pending_tag_reset(&mut pending_tag_reset, &line, &mut buffered_tag_resets) {
      continue;
    }

    // Buffer annotated tag blocks and emit once (rename/dedupe-safe)
    if line.starts_with(b"tag ") {
      crate::tag::process_tag_block(&line, &mut fe_out, &mut orig_file, &mut filt_file, if let Some(ref mut fi_in)=fi_in_opt { Some(fi_in) } else { None },
        &replacer, opts, &mut updated_refs, &mut annotated_tag_refs, &mut ref_renames, &mut emitted_marks)?;
      continue;
    }

    if line.starts_with(b"commit ") {
      // Start buffering a commit using possibly renamed header
      in_commit = true;
      commit_buf.clear();
      commit_has_changes = false;
      commit_mark = None;
      first_parent_mark = None;
      let hdr = crate::commit::rename_commit_header_ref(&line, opts, &mut ref_renames);
      commit_buf.extend_from_slice(&hdr);
      // Track final branch ref (post-rename) for HEAD updates
      let mut refname = &hdr[b"commit ".len()..];
      if let Some(&last) = refname.last() { if last == b'\n' { refname = &refname[..refname.len()-1]; } }
      if refname.starts_with(b"refs/heads/") { updated_branch_refs.insert(refname.to_vec()); }
      continue;
    }

    // If we are buffering a commit, process its content
    if in_commit {
        // End of commit is implicit: a new object starts.
        if line.starts_with(b"commit ") || line.starts_with(b"tag ") || line.starts_with(b"reset ") || line.starts_with(b"blob") || line == b"done\n" {
            match crate::commit::process_commit_line(
                b"\n", opts, &mut fe_out, &mut orig_file, &mut filt_file, if let Some(ref mut fi_in)=fi_in_opt { Some(fi_in) } else { None },
                &replacer, &mut commit_buf, &mut commit_has_changes, &mut commit_mark,
                &mut first_parent_mark, &mut commit_original_oid, &mut parent_count,
                &mut commit_pairs, &mut import_broken, &emitted_marks,
            )? {
                crate::commit::CommitAction::Consumed => {}, // Should not happen with synthetic newline
                crate::commit::CommitAction::Ended => {
                  // Record emitted commit mark
                  if let Some(m) = commit_mark { emitted_marks.insert(m); }
                  in_commit = false;
                }
            }
        }
    }
    if in_commit {
      // If the previous M-line declared inline content, handle its following data block here
      if line.starts_with(b"data ") {
        if let Some((pos, path_bytes)) = pending_inline.take() {
          // Parse size and read payload
          let size_bytes = &line[b"data ".len()..];
          let n = std::str::from_utf8(size_bytes)
            .ok().map(|s| s.trim()).and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
          let mut payload = vec![0u8; n];
          fe_out.read_exact(&mut payload)?;
          // Mirror original payload to debug file
          orig_file.write_all(&payload)?;
          let mut drop_inline = false;
          if let Some(max) = opts.max_blob_size { if n > max { drop_inline = true; } }
          if drop_inline {
            // Replace previously appended M inline line with a deletion
            commit_buf.truncate(pos);
            commit_buf.extend_from_slice(b"D ");
            commit_buf.extend_from_slice(&path_bytes);
            commit_buf.push(b'\n');
            commit_has_changes = true;
            // Record report sample for size-based strip
            if samples_size.len() < REPORT_SAMPLE_LIMIT && !samples_size.iter().any(|p| p == &path_bytes) {
              samples_size.push(path_bytes);
            }
            continue;
          } else {
            // Keep inline content: apply --replace-text (literal then regex) and append
            let mut new_payload = payload.clone();
            let mut changed = false;
            if let Some(r) = &content_replacer {
              let tmp = r.apply(new_payload.clone());
              changed |= tmp != new_payload;
              new_payload = tmp;
            }
            if let Some(rr) = &content_regex_replacer {
              let tmp = rr.apply_regex(new_payload.clone());
              changed |= tmp != new_payload;
              new_payload = tmp;
            }
            let header = format!("data {}\n", new_payload.len());
            commit_buf.extend_from_slice(header.as_bytes());
            commit_buf.extend_from_slice(&new_payload);
            if changed {
              if samples_modified.len() < REPORT_SAMPLE_LIMIT && !samples_modified.iter().any(|p| p == &path_bytes) {
                samples_modified.push(path_bytes.clone());
              }
              inline_modified_paths.insert(path_bytes.clone());
            }
            commit_has_changes = true;
            continue;
          }
        }
      }
      // Pre-check for oversized blobs referenced by this filechange
      if line.starts_with(b"M ") {
        // Detect inline and record path for the immediately following data block
        {
          let bytes = &line;
          // find end of mode and id
          let mut i = 2; // after 'M '
          while i < bytes.len() && bytes[i] != b' ' { i += 1; } // end of mode
          if i < bytes.len() { i += 1; }
          let id_start = i;
          while i < bytes.len() && bytes[i] != b' ' { i += 1; }
          let id_end = i;
          let path_start = if i < bytes.len() { i + 1 } else { bytes.len() };
          let id = &bytes[id_start..id_end];
          if id == b"inline" {
            // store commit_buf position before M-line is appended by process_commit_line, and raw path bytes (without trailing \n)
            let mut p = bytes[path_start..].to_vec();
            if let Some(last) = p.last() { if *last == b'\n' { p.pop(); } }
            pending_inline = Some((commit_buf.len(), p));
          }
        }
        if let Some(max) = opts.max_blob_size {
          // parse id and path
          let bytes = &line;
          // find end of mode and id
          let mut i = 2; // after 'M '
          while i < bytes.len() && bytes[i] != b' ' { i += 1; } // end of mode
          if i < bytes.len() { i += 1; }
          let id_start = i;
          while i < bytes.len() && bytes[i] != b' ' { i += 1; }
          let id_end = i;
          let path_start = if i < bytes.len() { i + 1 } else { bytes.len() };
          let id = &bytes[id_start..id_end];
          let mut drop_path = false;
          let mut reason_size = false;
          let mut reason_sha = false;
          if id.first().copied() == Some(b':') {
            // mark
            let mut num: u32 = 0; let mut seen=false; let mut j = 1;
            while j < id.len() { let b = id[j]; if b>=b'0'&&b<=b'9' { seen=true; num=num.saturating_mul(10).saturating_add((b-b'0') as u32);} else {break;} j+=1; }
            if seen && oversize_marks.contains(&num) {
              drop_path = true;
              // Record size sample path eagerly
              let path_bytes = &bytes[path_start..].to_vec();
              if samples_size.len() < REPORT_SAMPLE_LIMIT && !samples_size.iter().any(|p| p == path_bytes) { samples_size.push(path_bytes.clone()); }
              reason_size = suppressed_marks_by_size.contains(&num);
              reason_sha = suppressed_marks_by_sha.contains(&num);
            }
            if seen && modified_marks.contains(&num) {
              let path_bytes = &bytes[path_start..].to_vec();
              if samples_modified.len() < REPORT_SAMPLE_LIMIT && !samples_modified.iter().any(|p| p == path_bytes) { samples_modified.push(path_bytes.clone()); }
            }
          } else {
            // sha1
            if id.len() == 40 && id.iter().all(|b| (b'0'..=b'9').contains(b) || (b'a'..=b'f').contains(b)) {
              let sha = id.to_vec();
              if strip_sha_set.contains(&sha) { drop_path = true; reason_sha = true; suppressed_shas_by_sha.insert(sha.clone()); }
              let oversize = if let Some(sz) = sha_size_cache.get(&sha) { *sz > max } else {
                // query source repo for blob size
                let sha_str = String::from_utf8_lossy(&sha).to_string();
                let sz = Command::new("git")
                  .arg("-C").arg(&opts.source)
                  .arg("cat-file").arg("-s").arg(&sha_str)
                  .output()
                  .ok()
                  .and_then(|out| if out.status.success() {
                      std::str::from_utf8(&out.stdout).ok().and_then(|s| s.trim().parse::<usize>().ok())
                    } else { None })
                  .unwrap_or(0);
                sha_size_cache.insert(sha.clone(), sz);
                sz > max
              };
              if oversize {
                oversize_shas.insert(sha.clone()); suppressed_shas_by_size.insert(sha);
                drop_path = true; reason_size = true;
                // Record size sample path eagerly
                let path_bytes = &bytes[path_start..].to_vec();
                if samples_size.len() < REPORT_SAMPLE_LIMIT && !samples_size.iter().any(|p| p == path_bytes) { samples_size.push(path_bytes.clone()); }
              }
            }
          }
          if drop_path {
            // Emit deletion for the path
            let mut del = Vec::with_capacity(2 + bytes.len() - path_start);
            del.extend_from_slice(b"D ");
            del.extend_from_slice(&bytes[path_start..]);
            commit_buf.extend_from_slice(&del);
            commit_has_changes = true;
            let path_bytes = &bytes[path_start..].to_vec();
            let (mut r_size, mut r_sha) = (reason_size, reason_sha);
            if !r_size && !r_sha {
              if opts.max_blob_size.is_some() { r_size = true; } else { r_sha = true; }
            }
            if r_size {
              if samples_size.len() < REPORT_SAMPLE_LIMIT && !samples_size.iter().any(|p| p == path_bytes) { samples_size.push(path_bytes.clone()); }
            } else if r_sha {
              if samples_sha.len() < REPORT_SAMPLE_LIMIT && !samples_sha.iter().any(|p| p == path_bytes) { samples_sha.push(path_bytes.clone()); }
            }
            continue;
          }
        }
      }
      match crate::commit::process_commit_line(
        &line, opts, &mut fe_out, &mut orig_file, &mut filt_file, if let Some(ref mut fi_in)=fi_in_opt { Some(fi_in) } else { None },
        &replacer, &mut commit_buf, &mut commit_has_changes, &mut commit_mark,
        &mut first_parent_mark, &mut commit_original_oid, &mut parent_count,
        &mut commit_pairs, &mut import_broken, &emitted_marks,
      )? {
        crate::commit::CommitAction::Consumed => { continue; }
        crate::commit::CommitAction::Ended => { in_commit = false; }
      }
    }

    // Generic data blocks (e.g., blob): forward exact payload bytes
    if line.starts_with(b"data ") {
      let size_bytes = &line[b"data ".len()..];
      let n = std::str::from_utf8(size_bytes)
        .ok()
        .map(|s| s.trim())
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
      let mut payload = vec![0u8; n];
      fe_out.read_exact(&mut payload)?;
      // Always mirror to original
      orig_file.write_all(&payload)?;
      if in_blob {
        let mut skip_blob = false;
        let mut reason_size = false;
        let mut reason_sha = false;
        if let Some(max) = opts.max_blob_size {
          if n > max {
            // Pre-record oversize by mark/sha so commit M-lines using marks can be dropped later.
            if let Some(m) = last_blob_mark { oversize_marks.insert(m); suppressed_marks_by_size.insert(m); }
            if let Some(ref s) = last_blob_orig_sha { oversize_shas.insert(s.clone()); suppressed_shas_by_size.insert(s.clone()); }
            skip_blob = true; reason_size = true;
          }
        }
        if !skip_blob { if let Some(ref s) = last_blob_orig_sha { if strip_sha_set.contains(s) { skip_blob = true; reason_sha = true; } } }
        if skip_blob {
          if let Some(m) = last_blob_mark.take() {
            oversize_marks.insert(m);
            if reason_size { suppressed_marks_by_size.insert(m); } else if reason_sha { suppressed_marks_by_sha.insert(m); }
          }
          if let Some(sha) = last_blob_orig_sha.take() {
            oversize_shas.insert(sha.clone());
            if reason_size { suppressed_shas_by_size.insert(sha); } else if reason_sha { suppressed_shas_by_sha.insert(sha); }
          }
          in_blob = false; blob_buf.clear(); last_blob_mark = None;
          // Do not forward to filtered/import
          continue;
        } else {
          // Emit buffered blob header lines, then header and payload
          for h in blob_buf.drain(..) {
            filt_file.write_all(&h)?;
            if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&h) { if e.kind()==io::ErrorKind::BrokenPipe { import_broken=true; } else { return Err(e); } } }
          }
          // Forward header/payload (apply --replace-text if provided). Apply literal first, then optional regex; track whether modified.
          let mut new_payload = payload.clone();
          let mut changed = false;
          if let Some(r) = &content_replacer {
            let tmp = r.apply(new_payload.clone());
            changed |= tmp != new_payload;
            new_payload = tmp;
          }
          if let Some(rr) = &content_regex_replacer {
            let tmp = rr.apply_regex(new_payload.clone());
            changed |= tmp != new_payload;
            new_payload = tmp;
          }
          let header = format!("data {}\n", new_payload.len());
          filt_file.write_all(header.as_bytes())?;
          if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(header.as_bytes()) { if e.kind()==io::ErrorKind::BrokenPipe { import_broken=true; } else { return Err(e); } } }
          filt_file.write_all(&new_payload)?;
          if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&new_payload) { if e.kind()==io::ErrorKind::BrokenPipe { import_broken=true; } else { return Err(e); } } }
          if changed { if let Some(m) = last_blob_mark { modified_marks.insert(m); } }
          // Record emitted blob mark
          if let Some(m) = last_blob_mark { emitted_marks.insert(m); }
          in_blob = false; last_blob_mark = None;
          continue;
        }
      } else {
        // Not a blob payload (should be rare here); forward as-is
        filt_file.write_all(&line)?;
        if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&line) { if e.kind() == io::ErrorKind::BrokenPipe { import_broken = true; break; } else { return Err(e); } } }
        filt_file.write_all(&payload)?;
        if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&payload) { if e.kind() == io::ErrorKind::BrokenPipe { import_broken = true; break; } else { return Err(e); } } }
        continue;
      }
      // Do not consume or inject an extra newline; next header line follows in stream.
    }

    // Handle end-of-stream marker; flush buffered lightweight tag resets before 'done'
    if line == b"done\n" {
      crate::finalize::flush_lightweight_tag_resets(
        &mut buffered_tag_resets,
        &annotated_tag_refs,
        &mut filt_file,
        if let Some(ref mut fi_in) = fi_in_opt { Some(fi_in) } else { None },
        &mut import_broken,
      )?;
      // Forward 'done' after flushing
      filt_file.write_all(&line)?;
      if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&line) {
        if e.kind() == io::ErrorKind::BrokenPipe { import_broken = true; break; } else { return Err(e); }
      } }
      continue;
    }

    // Lightweight tag renames: reset refs/tags/<name>
    if crate::tag::process_reset_header(&line, opts, &mut ref_renames, &mut pending_tag_reset) { continue; }

    // Branch reset renames: reset refs/heads/<name>
    if line.starts_with(b"reset ") {
      let mut name = &line[b"reset ".len()..];
      if let Some(&last) = name.last() { if last == b'\n' { name = &name[..name.len()-1]; } }
      if name.starts_with(b"refs/heads/") {
        let mut out = line.clone();
        if let Some((ref old, ref new_)) = opts.branch_rename {
          let bname = &name[b"refs/heads/".len()..];
          if bname.starts_with(&old[..]) {
            let mut rebuilt = Vec::with_capacity(7 + b"refs/heads/".len() + new_.len() + (bname.len()-old.len()) + 1);
            rebuilt.extend_from_slice(b"reset ");
            rebuilt.extend_from_slice(b"refs/heads/");
            rebuilt.extend_from_slice(&new_);
            rebuilt.extend_from_slice(&bname[old.len()..]);
            rebuilt.push(b'\n');
            let new_full = [b"refs/heads/".as_ref(), new_.as_slice(), &bname[old.len()..]].concat();
            ref_renames.insert((name.to_vec(), new_full));
            out = rebuilt;
          }
        }
        // forward
        filt_file.write_all(&out)?;
        if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&out) { if e.kind()==io::ErrorKind::BrokenPipe { import_broken=true; } else { return Err(e); } } }
        continue;
      }
    }

    // Forward non-message lines as-is to filtered + import (drop stray blanks)
    if line == b"\n" { continue; }
    filt_file.write_all(&line)?;
    if let Some(ref mut fi_in) = fi_in_opt { if let Err(e) = fi_in.write_all(&line) {
      if e.kind() == io::ErrorKind::BrokenPipe { import_broken = true; break; } else { return Err(e); }
    } }
  }

  // Finalize run: flush buffered tags (if any remain), wait, write maps, optional reset
  let allow_flush_tag_resets = !buffered_tag_resets.is_empty();
  crate::finalize::finalize(
    opts,
    &debug_dir,
    ref_renames,
    commit_pairs,
    buffered_tag_resets,
    annotated_tag_refs,
    updated_branch_refs,
    &mut filt_file,
    fi_in_opt,
    &mut fe,
    fi.as_mut().map(|c| c),
    import_broken,
    allow_flush_tag_resets,
    {
      let mut size_cnt = suppressed_shas_by_size.len();
      if size_cnt == 0 { size_cnt = suppressed_marks_by_size.len(); }
      if size_cnt == 0 { size_cnt = samples_size.len(); }
      let mut sha_cnt = suppressed_shas_by_sha.len();
      if sha_cnt == 0 { sha_cnt = suppressed_marks_by_sha.len(); }
      Some(crate::finalize::ReportData {
        stripped_by_size: size_cnt,
        stripped_by_sha: sha_cnt,
        modified_blobs: modified_marks.len() + inline_modified_paths.len(),
        samples_size,
        samples_sha,
        samples_modified,
      })
    },
  )
}
