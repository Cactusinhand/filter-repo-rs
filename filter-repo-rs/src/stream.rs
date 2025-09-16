use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::process::{Command, Stdio};


use crate::gitutil::git_dir;
use crate::message::MessageReplacer;
use crate::message::blob_regex::RegexReplacer as BlobRegexReplacer;
use crate::opts::Options;

const REPORT_SAMPLE_LIMIT: usize = 20;
const SHA_HEX_LEN: usize = 40;
const SHA_BIN_LEN: usize = 20;
const STRIP_SHA_ON_DISK_THRESHOLD: usize = 100_000;

type ShaBytes = [u8; SHA_BIN_LEN];

static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

enum StripShaLookup {
  Empty,
  InMemory(Vec<ShaBytes>),
  OnDisk(TempSortedFile),
}

impl StripShaLookup {
  fn empty() -> Self { StripShaLookup::Empty }

  fn from_path(path: &Path) -> io::Result<Self> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries: Vec<ShaBytes> = Vec::new();
    for line in reader.lines() {
      let line = line?;
      if let Some(bytes) = parse_sha_line(&line) {
        entries.push(bytes);
      }
    }
    if entries.is_empty() { return Ok(StripShaLookup::Empty); }
    entries.sort_unstable();
    entries.dedup();
    if entries.len() > STRIP_SHA_ON_DISK_THRESHOLD {
      TempSortedFile::from_entries(entries).map(StripShaLookup::OnDisk)
    } else {
      Ok(StripShaLookup::InMemory(entries))
    }
  }

  fn contains_hex(&self, sha_hex: &[u8]) -> io::Result<bool> {
    if sha_hex.len() != SHA_HEX_LEN { return Ok(false); }
    let needle = match parse_sha_bytes(sha_hex) {
      Some(bytes) => bytes,
      None => return Ok(false),
    };
    match self {
      StripShaLookup::Empty => Ok(false),
      StripShaLookup::InMemory(entries) => Ok(entries.binary_search(&needle).is_ok()),
      StripShaLookup::OnDisk(file) => file.contains(&needle),
    }
  }
}

struct TempSortedFile {
  path: PathBuf,
  file: RefCell<File>,
  entries: u64,
}

impl TempSortedFile {
  fn from_entries(entries: Vec<ShaBytes>) -> io::Result<Self> {
    let count = entries.len() as u64;
    let (path, mut file) = create_temp_file("filter-repo-strip-sha")?;
    for entry in entries {
      file.write_all(&entry)?;
    }
    file.flush()?;
    file.seek(SeekFrom::Start(0))?;
    Ok(TempSortedFile { path, file: RefCell::new(file), entries: count })
  }

  fn contains(&self, needle: &ShaBytes) -> io::Result<bool> {
    let mut file = self.file.borrow_mut();
    let mut left: u64 = 0;
    let mut right: u64 = self.entries;
    let mut buf: ShaBytes = [0u8; SHA_BIN_LEN];
    while left < right {
      let mid = (left + right) / 2;
      file.seek(SeekFrom::Start(mid.saturating_mul(SHA_BIN_LEN as u64)))?;
      file.read_exact(&mut buf)?;
      match buf.cmp(needle) {
        Ordering::Less => left = mid + 1,
        Ordering::Greater => right = mid,
        Ordering::Equal => return Ok(true),
      }
    }
    Ok(false)
  }
}

impl Drop for TempSortedFile {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.path);
  }
}

fn create_temp_file(prefix: &str) -> io::Result<(PathBuf, File)> {
  let temp_dir = std::env::temp_dir();
  for attempt in 0..1000 {
    let counter = TEMP_FILE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed) + attempt;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let name = format!("{}-{}-{}", prefix, std::process::id(), timestamp + counter as u128);
    let path = temp_dir.join(name);
    match OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
      Ok(file) => return Ok((path, file)),
      Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
      Err(e) => return Err(e),
    }
  }
  Err(io::Error::new(io::ErrorKind::AlreadyExists, "failed to create temporary sha lookup file"))
}

fn parse_sha_line(line: &str) -> Option<ShaBytes> {
  parse_sha_bytes(line.trim().as_bytes())
}

fn parse_sha_bytes(bytes: &[u8]) -> Option<ShaBytes> {
  if bytes.len() != SHA_HEX_LEN { return None; }
  let mut out = [0u8; SHA_BIN_LEN];
  for (i, chunk) in bytes.chunks_exact(2).enumerate() {
    let hi = hex_val(chunk[0])?;
    let lo = hex_val(chunk[1])?;
    out[i] = (hi << 4) | lo;
  }
  Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
  match b {
    b'0'..=b'9' => Some(b - b'0'),
    b'a'..=b'f' => Some(b - b'a' + 10),
    b'A'..=b'F' => Some(b - b'A' + 10),
    _ => None,
  }
}

pub(crate) struct BlobSizeTracker {
  source: PathBuf,
  max_blob_size: Option<usize>,
  oversize: HashSet<Vec<u8>>,
  prefetch_ok: bool,
}

impl BlobSizeTracker {
  pub(crate) fn new(opts: &Options) -> Self {
    let mut tracker = BlobSizeTracker {
      source: opts.source.clone(),
      max_blob_size: opts.max_blob_size,
      oversize: HashSet::new(),
      prefetch_ok: false,
    };
    if opts.max_blob_size.is_some() {
      if let Err(e) = tracker.prefetch_oversize() {
        tracker.oversize.clear();
        if !opts.quiet {
          eprintln!(
            "Warning: batch blob size pre-computation failed ({e}), falling back to on-demand sizing"
          );
        }
      }
    }
    tracker
  }

  fn prefetch_oversize(&mut self) -> io::Result<()> {
    let max = match self.max_blob_size {
      Some(m) => m,
      None => return Ok(()),
    };
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(&self.source)
      .arg("cat-file")
      .arg("--batch-all-objects")
      .arg("--batch-check=%(objectname) %(objecttype) %(objectsize)")
      .stdout(Stdio::piped())
      .stderr(Stdio::piped());
    let mut child = cmd.spawn()
      .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to run git cat-file batch: {e}")))?;
    let stdout = child.stdout.take().ok_or_else(|| io::Error::new(io::ErrorKind::Other, "missing stdout from git cat-file batch"))?;
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::with_capacity(128);
    loop {
      line.clear();
      if reader.read_until(b'\n', &mut line)? == 0 { break; }
      if line.ends_with(b"\n") {
          line.pop();
          if line.ends_with(b"\r") {
              line.pop();
          }
      }
      if line.is_empty() { continue; }
      let mut it = line.split(|b| *b == b' ');
      let sha = match it.next() {
        Some(s) if !s.is_empty() => s,
        _ => continue,
      };
      let kind = match it.next() {
        Some(s) => s,
        None => continue,
      };
      if kind != b"blob" { continue; }
      let size_bytes = match it.next() {
        Some(s) => s,
        None => continue,
      };
      let size = std::str::from_utf8(size_bytes)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0);
      if size > max {
        self.oversize.insert(sha.to_vec());
      }
    }
    let mut stderr_buf = Vec::new();
    if let Some(mut err) = child.stderr.take() { err.read_to_end(&mut stderr_buf)?; }
    let status = child.wait()?;
    if !status.success() {
      let msg = String::from_utf8_lossy(&stderr_buf);
      return Err(io::Error::new(io::ErrorKind::Other, format!("git cat-file batch failed: {msg}")));
    }
    self.prefetch_ok = true;
    Ok(())
  }

  pub(crate) fn is_oversize(&mut self, sha: &[u8]) -> bool {
    let max = match self.max_blob_size {
      Some(m) => m,
      None => return false,
    };
    if self.oversize.contains(sha) { return true; }
    if self.prefetch_ok { return false; }
    let sha_str = String::from_utf8_lossy(sha).to_string();
    let output = Command::new("git")
      .arg("-C").arg(&self.source)
      .arg("cat-file").arg("-s").arg(&sha_str)
      .output();
    let size = match output {
      Ok(out) if out.status.success() => {
        std::str::from_utf8(&out.stdout).ok().and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0)
      }
      _ => 0,
    };
    if size > max {
      self.oversize.insert(sha.to_vec());
      true
    } else {
      false
    }
  }

  pub(crate) fn known_oversize(&self, sha: &[u8]) -> bool {
    self.oversize.contains(sha)
  }

  #[cfg(test)]
  pub(crate) fn prefetch_success(&self) -> bool { self.prefetch_ok }
}

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
  let strip_sha_lookup = match &opts.strip_blobs_with_ids {
    Some(path) => StripShaLookup::from_path(path)
      .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("failed to load --strip-blobs-with-ids: {e}")))?,
    None => StripShaLookup::empty(),
  };
  let mut last_blob_orig_sha: Option<Vec<u8>> = None;
  let mut blob_size_tracker = BlobSizeTracker::new(opts);
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
        } else if id.len() == 40 && id.iter().all(|b| b.is_ascii_hexdigit()) {
        // } else if id.len() == 40 && id.iter().all(|b| (b'0'..=b'9').contains(b) || (b'a'..=b'f').contains(b)) {
          // sha1
          let sha = id.to_vec();
          if strip_sha_lookup.contains_hex(&sha)? { drop_path = true; reason_sha = true; suppressed_shas_by_sha.insert(sha.clone()); }
          if blob_size_tracker.is_oversize(&sha) {
            oversize_shas.insert(sha.clone()); suppressed_shas_by_size.insert(sha);
            drop_path = true; reason_size = true;
            // Record size sample path eagerly
            let path_bytes = &bytes[path_start..].to_vec();
            if samples_size.len() < REPORT_SAMPLE_LIMIT && !samples_size.iter().any(|p| p == path_bytes) { samples_size.push(path_bytes.clone()); }
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
        if !skip_blob {
          if let Some(ref s) = last_blob_orig_sha {
            if strip_sha_lookup.contains_hex(s)? { skip_blob = true; reason_sha = true; }
          }
        }
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
    &blob_size_tracker,
  )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_opts(source: &str) -> Options {
        Options {
            source: PathBuf::from(source),
            target: PathBuf::from("."),
            refs: vec!["--all".to_string()],
            date_order: false,
            no_data: false,
            quiet: false,
            reset: true,
            replace_message_file: None,
            replace_text_file: None,
            paths: Vec::new(),
            invert_paths: false,
            path_globs: Vec::new(),
            path_renames: Vec::new(),
            tag_rename: None,
            branch_rename: None,
            max_blob_size: None,
            strip_blobs_with_ids: None,
            write_report: false,
            cleanup: crate::opts::CleanupMode::None,
            reencode: true,
            quotepath: true,
            mark_tags: true,
            fe_stream_override: None,
            force: false,
            enforce_sanity: false,
            dry_run: false,
            partial: false,
            sensitive: false,
            no_fetch: false,
        }
    }

    #[test]
    fn test_blob_size_tracker_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_str().unwrap();

        std::process::Command::new("git")
            .args(["init", "--bare", repo_path])
            .output()
            .unwrap();

        let mut opts = create_test_opts(repo_path);
        opts.max_blob_size = Some(1024);

        let tracker = BlobSizeTracker::new(&opts);
        assert!(tracker.prefetch_success());
        assert!(!tracker.known_oversize(b"0000000000000000000000000000000000000000"));
    }

    #[test]
    fn test_blob_size_tracker_detects_large_blob() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        std::process::Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .unwrap();

        let large_path = repo_path.join("large.bin");
        let small_path = repo_path.join("small.txt");
        std::fs::write(&large_path, vec![b'a'; 4096]).unwrap();
        std::fs::write(&small_path, b"hello").unwrap();

        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "commit", "-m", "add files"])
            .output()
            .unwrap();

        let ls_tree = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "ls-tree", "-r", "HEAD"])
            .output()
            .unwrap();
        let listing = String::from_utf8(ls_tree.stdout).unwrap();
        let mut large_sha = None;
        let mut small_sha = None;
        for line in listing.lines() {
            if let Some((meta, path)) = line.split_once('\t') {
                let mut parts = meta.split_whitespace();
                let _mode = parts.next();
                let kind = parts.next();
                let sha = parts.next();
                if let (Some("blob"), Some(sha_hex)) = (kind, sha) {
                    if path.ends_with("large.bin") {
                        large_sha = Some(sha_hex.as_bytes().to_vec());
                    } else if path.ends_with("small.txt") {
                        small_sha = Some(sha_hex.as_bytes().to_vec());
                    }
                }
            }
        }

        let large_sha = large_sha.expect("large blob sha");
        let small_sha = small_sha.expect("small blob sha");

        let mut opts = create_test_opts(repo_path.to_str().unwrap());
        opts.max_blob_size = Some(2048);
        let mut tracker = BlobSizeTracker::new(&opts);

        assert!(tracker.prefetch_success());
        assert!(tracker.known_oversize(&large_sha));
        assert!(!tracker.known_oversize(&small_sha));
        assert!(tracker.is_oversize(&large_sha));
        assert!(!tracker.is_oversize(&small_sha));
    }

    #[test]
    fn test_blob_size_tracker_handles_invalid_repo() {
        let mut opts = create_test_opts("/nonexistent/path");
        opts.max_blob_size = Some(100);

        let mut tracker = BlobSizeTracker::new(&opts);
        assert!(!tracker.prefetch_success());
        assert!(!tracker.is_oversize(b"0000000000000000000000000000000000000000"));
    }
}
