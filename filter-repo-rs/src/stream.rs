use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::commit::{AuthorRewriter, MailmapRewriter};
use crate::error::Result as FilterRepoResult;
use crate::gitutil::git_dir;
use crate::limits::parse_data_size_header;
use crate::message::blob_regex::RegexReplacer as BlobRegexReplacer;
use crate::message::msg_regex::RegexReplacer as MsgRegexReplacer;
use crate::message::{MessageReplacer, ShortHashMapper};
use crate::opts::Options;

const REPORT_SAMPLE_LIMIT: usize = 20;
const SHA_HEX_LEN: usize = 40;
const SHA_BIN_LEN: usize = 20;

fn rewrite_timestamp_line(line: &[u8], opts: &Options) -> Vec<u8> {
    if opts.date_shift.is_none() && opts.date_set.is_none() {
        return line.to_vec();
    }

    let line_str = match std::str::from_utf8(line) {
        Ok(s) => s,
        Err(_) => return line.to_vec(),
    };

    let prefix = if line_str.starts_with("author ") {
        "author "
    } else if line_str.starts_with("committer ") {
        "committer "
    } else {
        return line.to_vec();
    };

    let rest = &line_str[prefix.len()..];

    let email_end = match rest.rfind('>') {
        Some(pos) => pos,
        None => return line.to_vec(),
    };

    let after_email = &rest[email_end + 1..].trim_start();

    let mut parts = after_email.split_whitespace();
    let timestamp_str = match parts.next() {
        Some(t) => t,
        None => return line.to_vec(),
    };
    let timezone = match parts.next() {
        Some(tz) => tz,
        None => return line.to_vec(),
    };

    let timestamp: i64 = match timestamp_str.parse() {
        Ok(t) => t,
        Err(_) => return line.to_vec(),
    };

    let new_timestamp = if let Some(fixed_ts) = opts.date_set {
        fixed_ts
    } else if let Some(shift) = opts.date_shift {
        timestamp.saturating_add(shift)
    } else {
        timestamp
    };

    let identity_part = &rest[..email_end + 1];
    format!(
        "{}{} {} {}\n",
        prefix, identity_part, new_timestamp, timezone
    )
    .into_bytes()
}

fn rewrite_commit_identity_line(
    line: &[u8],
    opts: &Options,
    author_rewriter: Option<&AuthorRewriter>,
    committer_rewriter: Option<&AuthorRewriter>,
    email_rewriter: Option<&AuthorRewriter>,
    mailmap_rewriter: Option<&MailmapRewriter>,
) -> Vec<u8> {
    let is_author_line = line.starts_with(b"author ");
    let is_committer_line = line.starts_with(b"committer ");
    if !is_author_line && !is_committer_line {
        return line.to_vec();
    }

    let mut rewritten = line.to_vec();
    if let Some(mailmap) = mailmap_rewriter {
        rewritten = crate::commit::rewrite_mailmap_line(&rewritten, Some(mailmap));
    } else {
        if let Some(email_rw) = email_rewriter {
            rewritten = crate::commit::rewrite_email_line(&rewritten, Some(email_rw));
        }
        if is_author_line {
            if let Some(author_rw) = author_rewriter {
                rewritten = crate::commit::rewrite_author_line(&rewritten, Some(author_rw));
            }
        }
        if is_committer_line {
            if let Some(committer_rw) = committer_rewriter {
                rewritten = crate::commit::rewrite_author_line(&rewritten, Some(committer_rw));
            }
        }
    }

    rewrite_timestamp_line(&rewritten, opts)
}

/// Add a path sample to the collection if under limit and not already present.
fn add_sample(samples: &mut Vec<Vec<u8>>, path: &[u8]) {
    if samples.len() < REPORT_SAMPLE_LIMIT && !samples.iter().any(|p| p == path) {
        samples.push(path.to_vec());
    }
}

#[derive(Debug, Default)]
struct PathCompatStats {
    policy: String,
    sanitized: usize,
    skipped: usize,
    sanitized_samples: Vec<String>,
    skipped_samples: Vec<String>,
}

fn path_compat_sample_label(event: &crate::pathutil::PathCompatEvent) -> String {
    let original = crate::pathutil::format_path_bytes_for_report(&event.original);
    if let Some(ref rewritten) = event.rewritten {
        format!(
            "{} -> {} ({})",
            original,
            crate::pathutil::format_path_bytes_for_report(rewritten),
            event.reason
        )
    } else {
        format!("{} ({})", original, event.reason)
    }
}

fn record_path_compat_event(stats: &mut PathCompatStats, event: crate::pathutil::PathCompatEvent) {
    match event.action {
        crate::pathutil::PathCompatAction::Sanitized => {
            stats.sanitized += 1;
            if stats.sanitized_samples.len() < REPORT_SAMPLE_LIMIT {
                stats
                    .sanitized_samples
                    .push(path_compat_sample_label(&event));
            }
        }
        crate::pathutil::PathCompatAction::Skipped => {
            stats.skipped += 1;
            if stats.skipped_samples.len() < REPORT_SAMPLE_LIMIT {
                stats.skipped_samples.push(path_compat_sample_label(&event));
            }
        }
    }
}
/// Threshold for deciding whether to keep SHA lookup in memory or on disk.
/// When number of SHAs exceeds this, use disk-based sorted file.
/// Lowered from 50,000 to 10,000 to reduce memory spike during sorting
const STRIP_SHA_ON_DISK_THRESHOLD: usize = 10_000;

type ShaBytes = [u8; SHA_BIN_LEN];

static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

enum StripShaLookup {
    Empty,
    InMemory(Vec<ShaBytes>),
    OnDisk(TempSortedFile),
}

/// Loads and sorts SHA entries from a file.
///
/// NOTE: This implementation first loads all entries into memory, sorts them,
/// then decides whether to keep in memory or write to disk. For extremely large
/// SHA lists, this causes a memory spike.
///
/// OPTIMIZATION: Consider implementing external merge sort or chunked sorting
/// to reduce peak memory usage. The threshold STRIP_SHA_ON_DISK_THRESHOLD could
/// be lowered to trigger on-disk storage earlier, trading disk I/O for memory.
impl StripShaLookup {
    fn empty() -> Self {
        StripShaLookup::Empty
    }

    fn from_path(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut entries: Vec<ShaBytes> = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            match parse_sha_line(&line) {
                Some(bytes) => entries.push(bytes),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("invalid SHA entry in {}: {line}", path.display()),
                    ))
                }
            }
        }
        if entries.is_empty() {
            return Ok(StripShaLookup::Empty);
        }
        entries.sort_unstable();
        entries.dedup();
        if entries.len() > STRIP_SHA_ON_DISK_THRESHOLD {
            TempSortedFile::from_entries(entries).map(StripShaLookup::OnDisk)
        } else {
            Ok(StripShaLookup::InMemory(entries))
        }
    }

    fn contains_hex(&self, sha_hex: &[u8]) -> io::Result<bool> {
        if sha_hex.len() != SHA_HEX_LEN {
            return Ok(false);
        }
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
        Ok(TempSortedFile {
            path,
            file: RefCell::new(file),
            entries: count,
        })
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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            timestamp + counter as u128
        );
        let path = temp_dir.join(name);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to create temporary sha lookup file",
    ))
}

fn parse_sha_line(line: &str) -> Option<ShaBytes> {
    parse_sha_bytes(line.trim().as_bytes())
}

fn parse_sha_bytes(bytes: &[u8]) -> Option<ShaBytes> {
    if bytes.len() != SHA_HEX_LEN {
        return None;
    }
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

fn process_blob_content(
    payload: Vec<u8>,
    content_replacer: &Option<MessageReplacer>,
    content_regex_replacer: &Option<BlobRegexReplacer>,
) -> (Vec<u8>, bool) {
    if content_replacer.is_none() && content_regex_replacer.is_none() {
        return (payload, false);
    }

    let mut data = payload;
    let mut changed = false;
    if let Some(r) = content_replacer {
        let (tmp, did_change) = r.apply_with_change(data);
        changed = did_change;
        data = tmp;
    }
    if let Some(rr) = content_regex_replacer {
        let (tmp, did_change) = rr.apply_regex_with_change(data);
        changed = changed || did_change;
        data = tmp;
    }
    (data, changed)
}

/// Tracks which marks/shas were filtered and why (size vs sha-strip).
struct FilterTracker {
    oversize_marks: HashSet<u32>,
    oversize_shas: HashSet<Vec<u8>>,
    suppressed_marks_by_size: HashSet<u32>,
    suppressed_marks_by_sha: HashSet<u32>,
    suppressed_shas_by_size: HashSet<Vec<u8>>,
    suppressed_shas_by_sha: HashSet<Vec<u8>>,
    modified_marks: HashSet<u32>,
    emitted_marks: HashSet<u32>,
}

impl FilterTracker {
    fn new() -> Self {
        Self {
            oversize_marks: HashSet::new(),
            oversize_shas: HashSet::new(),
            suppressed_marks_by_size: HashSet::new(),
            suppressed_marks_by_sha: HashSet::new(),
            suppressed_shas_by_size: HashSet::new(),
            suppressed_shas_by_sha: HashSet::new(),
            modified_marks: HashSet::new(),
            emitted_marks: HashSet::new(),
        }
    }
}

/// Accumulates sample paths for the final report.
struct ReportSamples {
    size: Vec<Vec<u8>>,
    sha: Vec<Vec<u8>>,
    modified: Vec<Vec<u8>>,
    inline_modified_paths: HashSet<Vec<u8>>,
}

impl ReportSamples {
    fn new() -> Self {
        Self {
            size: Vec::new(),
            sha: Vec::new(),
            modified: Vec::new(),
            inline_modified_paths: HashSet::new(),
        }
    }
}

struct PendingInlineDataCtx<'a> {
    opts: &'a Options,
    fe_out: &'a mut BufReader<std::process::ChildStdout>,
    orig_file_opt: &'a mut Option<BufWriter<File>>,
    commit_buf: &'a mut Vec<u8>,
    commit_has_changes: &'a mut bool,
    pending_inline: &'a mut Option<(usize, Vec<u8>)>,
    samples: &'a mut ReportSamples,
    path_compat_stats: &'a mut PathCompatStats,
    content_replacer: &'a Option<MessageReplacer>,
    content_regex_replacer: &'a Option<BlobRegexReplacer>,
}

fn process_pending_inline_data_line(
    line: &[u8],
    ctx: &mut PendingInlineDataCtx<'_>,
) -> FilterRepoResult<bool> {
    if !line.starts_with(b"data ") {
        return Ok(false);
    }
    let Some((pos, path_bytes)) = ctx.pending_inline.take() else {
        return Ok(false);
    };

    let n = parse_data_size_header(line)?;
    let mut payload = vec![0u8; n];
    ctx.fe_out.read_exact(&mut payload)?;
    if let Some(ref mut f) = ctx.orig_file_opt {
        f.write_all(&payload)?;
    }

    let mut drop_inline = false;
    if let Some(max) = ctx.opts.max_blob_size {
        if n > max {
            drop_inline = true;
        }
    }
    if drop_inline {
        ctx.commit_buf.truncate(pos);
        let decoded = crate::pathutil::decode_fast_export_path_bytes(&path_bytes);
        let (enc, path_event) =
            crate::pathutil::encode_path_for_fi_with_policy(&decoded, ctx.opts.path_compat_policy)
                .map_err(io::Error::other)?;
        if let Some(event) = path_event {
            record_path_compat_event(ctx.path_compat_stats, event);
        }
        if let Some(enc) = enc {
            ctx.commit_buf.extend_from_slice(b"D ");
            ctx.commit_buf.extend_from_slice(&enc);
            ctx.commit_buf.push(b'\n');
            *ctx.commit_has_changes = true;
        }
        add_sample(&mut ctx.samples.size, &path_bytes);
        return Ok(true);
    }

    if ctx.content_replacer.is_none() && ctx.content_regex_replacer.is_none() {
        let header = format!("data {}\n", payload.len());
        ctx.commit_buf.extend_from_slice(header.as_bytes());
        ctx.commit_buf.extend_from_slice(&payload);
    } else {
        let mut new_payload = payload;
        let mut changed = false;
        if let Some(r) = ctx.content_replacer {
            let (tmp, did_change) = r.apply_with_change(new_payload);
            changed = changed || did_change;
            new_payload = tmp;
        }
        if let Some(rr) = ctx.content_regex_replacer {
            let (tmp, did_change) = rr.apply_regex_with_change(new_payload);
            changed = changed || did_change;
            new_payload = tmp;
        }
        let header = format!("data {}\n", new_payload.len());
        ctx.commit_buf.extend_from_slice(header.as_bytes());
        ctx.commit_buf.extend_from_slice(&new_payload);
        if changed {
            add_sample(&mut ctx.samples.modified, &path_bytes);
            ctx.samples.inline_modified_paths.insert(path_bytes.clone());
        }
    }
    *ctx.commit_has_changes = true;
    Ok(true)
}

fn parse_commit_m_line_id_and_path(line: &[u8]) -> (&[u8], &[u8]) {
    let mut i = 2;
    while i < line.len() && line[i] != b' ' {
        i += 1;
    }
    if i < line.len() {
        i += 1;
    }
    let id_start = i;
    while i < line.len() && line[i] != b' ' {
        i += 1;
    }
    let id_end = i;
    let path_start = if i < line.len() { i + 1 } else { line.len() };
    (&line[id_start..id_end], &line[path_start..])
}

struct CommitMPrecheckCtx<'a> {
    opts: &'a Options,
    commit_buf: &'a mut Vec<u8>,
    commit_has_changes: &'a mut bool,
    pending_inline: &'a mut Option<(usize, Vec<u8>)>,
    tracker: &'a mut FilterTracker,
    samples: &'a mut ReportSamples,
    path_compat_stats: &'a mut PathCompatStats,
    strip_sha_lookup: &'a StripShaLookup,
    blob_size_tracker: &'a mut BlobSizeTracker,
}

fn process_commit_m_line_precheck(
    line: &[u8],
    ctx: &mut CommitMPrecheckCtx<'_>,
) -> FilterRepoResult<bool> {
    let opts = ctx.opts;
    let tracker = &mut *ctx.tracker;
    let samples = &mut *ctx.samples;

    let (id, path_bytes) = parse_commit_m_line_id_and_path(line);
    if id == b"inline" {
        let mut p = path_bytes.to_vec();
        if let Some(last) = p.last() {
            if *last == b'\n' {
                p.pop();
            }
        }
        *ctx.pending_inline = Some((ctx.commit_buf.len(), p));
    }

    let mut drop_path = false;
    let mut reason_size = false;
    let mut reason_sha = false;
    if id.first().copied() == Some(b':') {
        let mut num: u32 = 0;
        let mut seen = false;
        let mut j = 1;
        while j < id.len() {
            let b = id[j];
            if b.is_ascii_digit() {
                seen = true;
                num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
            } else {
                break;
            }
            j += 1;
        }
        if seen && tracker.oversize_marks.contains(&num) {
            drop_path = true;
            add_sample(&mut samples.size, path_bytes);
            reason_size = tracker.suppressed_marks_by_size.contains(&num);
            reason_sha = tracker.suppressed_marks_by_sha.contains(&num);
        }
        if seen && tracker.modified_marks.contains(&num) {
            add_sample(&mut samples.modified, path_bytes);
        }
    } else if id.len() == 40 && id.iter().all(|b| b.is_ascii_hexdigit()) {
        let sha = id.to_vec();
        if ctx.strip_sha_lookup.contains_hex(&sha)? {
            drop_path = true;
            reason_sha = true;
            tracker.suppressed_shas_by_sha.insert(sha.clone());
        }
        if ctx.blob_size_tracker.is_oversize(&sha) {
            tracker.oversize_shas.insert(sha.clone());
            tracker.suppressed_shas_by_size.insert(sha);
            drop_path = true;
            reason_size = true;
            let path_buf = path_bytes.to_vec();
            if samples.size.len() < REPORT_SAMPLE_LIMIT
                && !samples.size.iter().any(|p| p == &path_buf)
            {
                samples.size.push(path_buf);
            }
        }
    }

    if !drop_path {
        return Ok(false);
    }

    let decoded = crate::pathutil::decode_fast_export_path_bytes(path_bytes);
    let (enc, path_event) =
        crate::pathutil::encode_path_for_fi_with_policy(&decoded, opts.path_compat_policy)
            .map_err(io::Error::other)?;
    if let Some(event) = path_event {
        record_path_compat_event(ctx.path_compat_stats, event);
    }
    if let Some(enc) = enc {
        ctx.commit_buf.extend_from_slice(b"D ");
        ctx.commit_buf.extend_from_slice(&enc);
        ctx.commit_buf.push(b'\n');
        *ctx.commit_has_changes = true;
    }
    let (mut r_size, mut r_sha) = (reason_size, reason_sha);
    if !r_size && !r_sha {
        if opts.max_blob_size.is_some() {
            r_size = true;
        } else {
            r_sha = true;
        }
    }
    if r_size {
        add_sample(&mut samples.size, path_bytes);
    } else if r_sha {
        add_sample(&mut samples.sha, path_bytes);
    }
    Ok(true)
}

struct BlobPayloadCtx<'a> {
    opts: &'a Options,
    filt_file: &'a mut BufWriter<File>,
    fi_in_opt: &'a mut Option<BufWriter<std::process::ChildStdin>>,
    content_replacer: &'a Option<MessageReplacer>,
    content_regex_replacer: &'a Option<BlobRegexReplacer>,
    in_blob: &'a mut bool,
    blob_buf: &'a mut Vec<Vec<u8>>,
    last_blob_mark: &'a mut Option<u32>,
    last_blob_orig_sha: &'a mut Option<Vec<u8>>,
    tracker: &'a mut FilterTracker,
    import_broken: &'a mut bool,
    strip_sha_lookup: &'a StripShaLookup,
}

fn process_blob_data_payload(
    payload: Vec<u8>,
    ctx: &mut BlobPayloadCtx<'_>,
) -> FilterRepoResult<()> {
    let opts = ctx.opts;
    let tracker = &mut *ctx.tracker;

    let n = payload.len();
    let mut skip_blob = false;
    let mut reason_size = false;
    let mut reason_sha = false;
    if let Some(max) = opts.max_blob_size {
        if n > max {
            if let Some(m) = *ctx.last_blob_mark {
                tracker.oversize_marks.insert(m);
                tracker.suppressed_marks_by_size.insert(m);
            }
            if let Some(ref s) = *ctx.last_blob_orig_sha {
                tracker.oversize_shas.insert(s.clone());
                tracker.suppressed_shas_by_size.insert(s.clone());
            }
            skip_blob = true;
            reason_size = true;
        }
    }
    if !skip_blob {
        if let Some(ref s) = *ctx.last_blob_orig_sha {
            if ctx.strip_sha_lookup.contains_hex(s)? {
                skip_blob = true;
                reason_sha = true;
            }
        }
    }
    if skip_blob {
        if let Some(m) = ctx.last_blob_mark.take() {
            tracker.oversize_marks.insert(m);
            if reason_size {
                tracker.suppressed_marks_by_size.insert(m);
            } else if reason_sha {
                tracker.suppressed_marks_by_sha.insert(m);
            }
        }
        if let Some(sha) = ctx.last_blob_orig_sha.take() {
            tracker.oversize_shas.insert(sha.clone());
            if reason_size {
                tracker.suppressed_shas_by_size.insert(sha);
            } else if reason_sha {
                tracker.suppressed_shas_by_sha.insert(sha);
            }
        }
        *ctx.in_blob = false;
        ctx.blob_buf.clear();
        *ctx.last_blob_mark = None;
        return Ok(());
    }

    for h in ctx.blob_buf.drain(..) {
        ctx.filt_file.write_all(&h)?;
        if let Some(ref mut fi_in) = ctx.fi_in_opt {
            if let Err(e) = fi_in.write_all(&h) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *ctx.import_broken = true;
                } else {
                    return Err(e.into());
                }
            }
        }
    }
    if ctx.content_replacer.is_none() && ctx.content_regex_replacer.is_none() {
        let header = format!("data {}\n", n);
        ctx.filt_file.write_all(header.as_bytes())?;
        if let Some(ref mut fi_in) = ctx.fi_in_opt {
            if let Err(e) = fi_in.write_all(header.as_bytes()) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *ctx.import_broken = true;
                } else {
                    return Err(e.into());
                }
            }
        }
        ctx.filt_file.write_all(&payload)?;
        if let Some(ref mut fi_in) = ctx.fi_in_opt {
            if let Err(e) = fi_in.write_all(&payload) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *ctx.import_broken = true;
                } else {
                    return Err(e.into());
                }
            }
        }
    } else {
        let (new_payload, changed) =
            process_blob_content(payload, ctx.content_replacer, ctx.content_regex_replacer);
        let header = format!("data {}\n", new_payload.len());
        ctx.filt_file.write_all(header.as_bytes())?;
        if let Some(ref mut fi_in) = ctx.fi_in_opt {
            if let Err(e) = fi_in.write_all(header.as_bytes()) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *ctx.import_broken = true;
                } else {
                    return Err(e.into());
                }
            }
        }
        ctx.filt_file.write_all(&new_payload)?;
        if let Some(ref mut fi_in) = ctx.fi_in_opt {
            if let Err(e) = fi_in.write_all(&new_payload) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    *ctx.import_broken = true;
                } else {
                    return Err(e.into());
                }
            }
        }
        if changed {
            if let Some(m) = *ctx.last_blob_mark {
                tracker.modified_marks.insert(m);
            }
        }
    }
    if let Some(m) = *ctx.last_blob_mark {
        tracker.emitted_marks.insert(m);
    }
    *ctx.in_blob = false;
    *ctx.last_blob_mark = None;

    Ok(())
}

pub(crate) struct BlobSizeTracker {
    source: PathBuf,
    max_blob_size: Option<usize>,
    oversize: HashSet<Vec<u8>>,
    prefetch_ok: bool,
    batch: Option<BatchCat>,
}

impl BlobSizeTracker {
    pub(crate) fn new(opts: &Options) -> Self {
        let mut tracker = BlobSizeTracker {
            source: opts.source.clone(),
            max_blob_size: opts.max_blob_size,
            oversize: HashSet::new(),
            prefetch_ok: false,
            batch: None,
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

    fn ensure_batch(&mut self) -> io::Result<()> {
        if self.batch.is_some() {
            return Ok(());
        }
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&self.source)
            .arg("cat-file")
            .arg("--batch-check=%(objectname) %(objecttype) %(objectsize)")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| {
            io::Error::other(format!("failed to spawn git cat-file --batch-check: {e}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing stdin for cat-file batch"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout for cat-file batch"))?;
        self.batch = Some(BatchCat {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        });
        Ok(())
    }

    fn query_size_via_batch(&mut self, sha: &[u8]) -> io::Result<usize> {
        self.ensure_batch()?;
        let batch = self
            .batch
            .as_mut()
            .ok_or_else(|| io::Error::other("batch process not initialized before query"))?;
        // Write request (sha + newline)
        batch.stdin.write_all(sha)?;
        batch.stdin.write_all(b"\n")?;
        batch.stdin.flush()?;
        // Read one line: "<sha> <type> <size>"
        let mut line = Vec::with_capacity(64);
        line.clear();
        batch.stdout.read_until(b'\n', &mut line)?;
        // Trim CRLF
        while line.last().copied() == Some(b'\n') || line.last().copied() == Some(b'\r') {
            line.pop();
        }
        let mut it = line.split(|b| *b == b' ');
        let _sha_out = it.next();
        let kind = it.next().unwrap_or(b"");
        if kind != b"blob" {
            // Non-blob objects are not counted towards size limits
            return Ok(0);
        }
        let size_bytes = it.next().unwrap_or(b"0");
        let size = std::str::from_utf8(size_bytes)
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or_else(|| {
                eprintln!("WARNING: failed to parse blob size for {:?}", sha);
                0
            });
        Ok(size)
    }

    fn prefetch_oversize(&mut self) -> io::Result<()> {
        let max = match self.max_blob_size {
            Some(m) => m,
            None => return Ok(()),
        };
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&self.source)
            .arg("cat-file")
            .arg("--batch-all-objects")
            .arg("--batch-check=%(objectname) %(objecttype) %(objectsize)")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| io::Error::other(format!("failed to run git cat-file batch: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout from git cat-file batch"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = Vec::with_capacity(128);
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.ends_with(b"\n") {
                line.pop();
                if line.ends_with(b"\r") {
                    line.pop();
                }
            }
            if line.is_empty() {
                continue;
            }
            let mut it = line.split(|b| *b == b' ');
            let sha = match it.next() {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let kind = match it.next() {
                Some(s) => s,
                None => continue,
            };
            if kind != b"blob" {
                continue;
            }
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
        if let Some(mut err) = child.stderr.take() {
            err.read_to_end(&mut stderr_buf)?;
        }
        let status = child.wait()?;
        if !status.success() {
            let msg = String::from_utf8_lossy(&stderr_buf);
            return Err(io::Error::other(format!(
                "git cat-file batch failed: {msg}"
            )));
        }
        self.prefetch_ok = true;
        Ok(())
    }

    pub(crate) fn is_oversize(&mut self, sha: &[u8]) -> bool {
        let max = match self.max_blob_size {
            Some(m) => m,
            None => return false,
        };
        if self.oversize.contains(sha) {
            return true;
        }
        if self.prefetch_ok {
            return false;
        }
        let size = self.query_size_via_batch(sha).unwrap_or_default();
        if size > max {
            self.oversize.insert(sha.to_vec());
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn known_oversize(&self, sha: &[u8]) -> bool {
        self.oversize.contains(sha)
    }

    #[cfg(test)]
    pub(crate) fn prefetch_success(&self) -> bool {
        self.prefetch_ok
    }
}

struct BatchCat {
    child: std::process::Child,
    stdin: BufWriter<std::process::ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Drop for BatchCat {
    fn drop(&mut self) {
        // Best-effort shutdown
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct StreamProcessor<'a> {
    opts: &'a Options,
    debug_dir: PathBuf,
}

impl<'a> StreamProcessor<'a> {
    fn new(opts: &'a Options) -> io::Result<Self> {
        let target_git_dir = git_dir(&opts.target).map_err(|e| {
            io::Error::other(format!("Target {:?} is not a git repo: {e}", opts.target))
        })?;
        let _ = git_dir(&opts.source).map_err(|e| {
            io::Error::other(format!("Source {:?} is not a git repo: {e}", opts.source))
        })?;

        let debug_dir = target_git_dir.join("filter-repo");
        if !debug_dir.exists() {
            create_dir_all(&debug_dir)?;
        }

        Ok(Self { opts, debug_dir })
    }

    fn init_stream_io(
        &self,
    ) -> io::Result<(
        BufWriter<File>,
        Option<BufWriter<File>>,
        std::process::Child,
        Option<std::process::Child>,
        BufReader<std::process::ChildStdout>,
        Option<BufWriter<std::process::ChildStdin>>,
        Option<BufReader<std::process::ChildStdout>>,
    )> {
        let opts = self.opts;
        let debug_dir = &self.debug_dir;
        let filt_file = BufWriter::new(File::create(debug_dir.join("fast-export.filtered"))?);
        let write_original = opts.debug_mode || opts.write_report;
        let orig_file_opt: Option<BufWriter<File>> = if write_original {
            Some(BufWriter::new(File::create(
                debug_dir.join("fast-export.original"),
            )?))
        } else {
            None
        };

        let mut fe_cmd = crate::pipes::build_fast_export_cmd(opts)?;
        let mut fe = fe_cmd
            .spawn()
            .map_err(|e| io::Error::other(format!("failed to spawn git fast-export: {e}")))?;
        let mut fi = if opts.dry_run {
            None
        } else {
            Some(
                crate::pipes::build_fast_import_cmd(opts)
                    .spawn()
                    .map_err(|e| {
                        io::Error::other(format!("failed to spawn git fast-import: {e}"))
                    })?,
            )
        };

        let fe_out = BufReader::new(
            fe.stdout
                .take()
                .ok_or_else(|| io::Error::other("git fast-export produced no stdout"))?,
        );
        let fi_in_opt: Option<BufWriter<std::process::ChildStdin>> = if let Some(ref mut child) = fi
        {
            child.stdin.take().map(BufWriter::new)
        } else {
            None
        };
        let fi_out_opt: Option<BufReader<std::process::ChildStdout>> =
            if let Some(ref mut child) = fi {
                child.stdout.take().map(BufReader::new)
            } else {
                None
            };

        Ok((
            filt_file,
            orig_file_opt,
            fe,
            fi,
            fe_out,
            fi_in_opt,
            fi_out_opt,
        ))
    }

    fn init_rewriters(
        &self,
    ) -> io::Result<(
        Option<MessageReplacer>,
        Option<MsgRegexReplacer>,
        Option<ShortHashMapper>,
        Option<MessageReplacer>,
        Option<BlobRegexReplacer>,
        Option<AuthorRewriter>,
        Option<AuthorRewriter>,
        Option<AuthorRewriter>,
        Option<MailmapRewriter>,
    )> {
        let opts = self.opts;
        let debug_dir = &self.debug_dir;

        let replacer =
            match &opts.replace_message_file {
                Some(p) => Some(MessageReplacer::from_file(p).map_err(|e| {
                    io::Error::other(format!("failed to read --replace-message: {e}"))
                })?),
                None => None,
            };
        let msg_regex_replacer: Option<MsgRegexReplacer> = match &opts.replace_message_file {
            Some(p) => MsgRegexReplacer::from_file(p)
                .map_err(|e| io::Error::other(format!("failed to read --replace-message: {e}")))?,
            None => None,
        };
        let short_hash_mapper = ShortHashMapper::from_debug_dir(debug_dir)?;
        let content_replacer = match &opts.replace_text_file {
            Some(p) => Some(
                MessageReplacer::from_file(p)
                    .map_err(|e| io::Error::other(format!("failed to read --replace-text: {e}")))?,
            ),
            None => None,
        };
        let content_regex_replacer: Option<BlobRegexReplacer> = match &opts.replace_text_file {
            Some(p) => BlobRegexReplacer::from_file(p)
                .map_err(|e| io::Error::other(format!("failed to read --replace-text: {e}")))?,
            None => None,
        };

        let author_rewriter =
            match &opts.author_rewrite_file {
                Some(p) => Some(AuthorRewriter::from_file(p).map_err(|e| {
                    io::Error::other(format!("failed to read --author-rewrite: {e}"))
                })?),
                None => None,
            };
        let committer_rewriter = match &opts.committer_rewrite_file {
            Some(p) => Some(AuthorRewriter::from_file(p).map_err(|e| {
                io::Error::other(format!("failed to read --committer-rewrite: {e}"))
            })?),
            None => None,
        };
        let email_rewriter =
            match &opts.email_rewrite_file {
                Some(p) => Some(AuthorRewriter::from_file(p).map_err(|e| {
                    io::Error::other(format!("failed to read --email-rewrite: {e}"))
                })?),
                None => None,
            };
        let mailmap_rewriter = match &opts.mailmap_file {
            Some(p) => Some(
                MailmapRewriter::from_file(p)
                    .map_err(|e| io::Error::other(format!("failed to read --mailmap: {e}")))?,
            ),
            None => None,
        };

        Ok((
            replacer,
            msg_regex_replacer,
            short_hash_mapper,
            content_replacer,
            content_regex_replacer,
            author_rewriter,
            committer_rewriter,
            email_rewriter,
            mailmap_rewriter,
        ))
    }

    fn finalize_stream(
        &self,
        opts: &Options,
        filt_file: &mut BufWriter<File>,
        orig_file_opt: &mut Option<BufWriter<File>>,
        fi_in_opt: &mut Option<BufWriter<std::process::ChildStdin>>,
        fe: &mut std::process::Child,
        fi: &mut Option<std::process::Child>,
        import_broken: bool,
        ref_renames: BTreeSet<(Vec<u8>, Vec<u8>)>,
        commit_pairs: Vec<(Vec<u8>, Option<u32>)>,
        buffered_tag_resets: Vec<(Vec<u8>, Vec<u8>)>,
        annotated_tag_refs: BTreeSet<Vec<u8>>,
        updated_branch_refs: BTreeSet<Vec<u8>>,
        branch_reset_targets: Vec<(Vec<u8>, Vec<u8>)>,
        tracker: FilterTracker,
        samples: ReportSamples,
        total_commits: usize,
        total_blobs: usize,
        path_compat_stats: PathCompatStats,
        blob_size_tracker: &BlobSizeTracker,
    ) -> FilterRepoResult<()> {
        if let Some(ref mut of) = orig_file_opt {
            of.flush()?;
        }
        let allow_flush_tag_resets = !buffered_tag_resets.is_empty();
        let fi_writer_for_finalize: Option<Box<dyn Write>> =
            fi_in_opt.take().map(|bw| Box::new(bw) as Box<dyn Write>);
        let total_refs_rewritten = ref_renames.len();

        crate::finalize::finalize(
            opts,
            &self.debug_dir,
            ref_renames,
            commit_pairs,
            buffered_tag_resets,
            annotated_tag_refs,
            updated_branch_refs,
            branch_reset_targets,
            filt_file as &mut dyn Write,
            fi_writer_for_finalize,
            fe,
            fi.as_mut(),
            import_broken,
            allow_flush_tag_resets,
            {
                use crate::finalize::{
                    Metadata, ReportData, Samples, Statistics, Summary, WindowsPathReport,
                    WindowsPathSamples, WindowsPathSummary,
                };
                Some(ReportData {
                    summary: Summary {
                        blobs_stripped_by_size: tracker
                            .suppressed_shas_by_size
                            .len()
                            .max(tracker.suppressed_marks_by_size.len()),
                        blobs_stripped_by_sha: tracker
                            .suppressed_shas_by_sha
                            .len()
                            .max(tracker.suppressed_marks_by_sha.len()),
                        blobs_modified: tracker.modified_marks.len()
                            + samples.inline_modified_paths.len(),
                    },
                    statistics: Statistics {
                        commits_processed: total_commits,
                        blobs_processed: total_blobs,
                        refs_rewritten: total_refs_rewritten,
                    },
                    samples: Samples {
                        by_size: samples
                            .size
                            .into_iter()
                            .map(|p| String::from_utf8_lossy(&p).into_owned())
                            .collect(),
                        by_sha: samples
                            .sha
                            .into_iter()
                            .map(|p| String::from_utf8_lossy(&p).into_owned())
                            .collect(),
                        modified: samples
                            .modified
                            .into_iter()
                            .map(|p| String::from_utf8_lossy(&p).into_owned())
                            .collect(),
                    },
                    windows_path: if path_compat_stats.sanitized + path_compat_stats.skipped > 0 {
                        Some(WindowsPathReport {
                            summary: WindowsPathSummary {
                                policy: path_compat_stats.policy,
                                sanitized: path_compat_stats.sanitized,
                                skipped: path_compat_stats.skipped,
                            },
                            samples: if path_compat_stats.sanitized_samples.is_empty()
                                && path_compat_stats.skipped_samples.is_empty()
                            {
                                None
                            } else {
                                Some(WindowsPathSamples {
                                    sanitized: path_compat_stats.sanitized_samples,
                                    skipped: path_compat_stats.skipped_samples,
                                })
                            },
                        })
                    } else {
                        None
                    },
                    metadata: Metadata {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs().to_string())
                            .unwrap_or_default(),
                    },
                })
            },
            blob_size_tracker,
        )?;

        Ok(())
    }

    fn process(&self) -> FilterRepoResult<()> {
        let opts = self.opts;
        let (
            mut filt_file,
            mut orig_file_opt,
            mut fe,
            mut fi,
            mut fe_out,
            mut fi_in_opt,
            mut fi_out_opt,
        ) = self.init_stream_io()?;
        let (
            replacer,
            msg_regex_replacer,
            mut short_hash_mapper,
            content_replacer,
            content_regex_replacer,
            author_rewriter,
            committer_rewriter,
            email_rewriter,
            mailmap_rewriter,
        ) = self.init_rewriters()?;

        // minimal stream state is tracked via local booleans and buffers
        // Commit buffering state for pruning
        let mut in_commit = false;
        let mut commit_buf: Vec<u8> = Vec::with_capacity(8192);
        let mut commit_has_changes = false;
        let mut commit_mark: Option<u32> = None;
        let mut first_parent_mark: Option<u32> = None;
        let mut commit_original_oid: Option<Vec<u8>> = None;
        let mut parent_count: usize = 0;
        let mut commit_pairs: Vec<(Vec<u8>, Option<u32>)> = Vec::new();
        let mut parent_lines: Vec<crate::commit::ParentLine> = Vec::new();
        let mut alias_map: HashMap<u32, u32> = HashMap::new();
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
        // Track branch reset targets to feed finalize phase (ref -> mark/oid spec)
        let mut branch_reset_targets: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        // Buffer lightweight tag resets (ref, from-line)
        let mut buffered_tag_resets: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        // After seeing a reset refs/tags/<name>, capture the following 'from ...' line
        let mut pending_tag_reset: Option<Vec<u8>> = None;
        // After seeing a branch reset, capture the following 'from ...' line
        let mut pending_branch_reset: Option<Vec<u8>> = None;
        // Blob filtering state for --max-blob-size
        let mut in_blob: bool = false;
        let mut blob_buf: Vec<Vec<u8>> = Vec::new();
        let mut last_blob_mark: Option<u32> = None;
        let strip_sha_lookup = match &opts.strip_blobs_with_ids {
            Some(path) => StripShaLookup::from_path(path).map_err(|e| {
                io::Error::other(format!("failed to load --strip-blobs-with-ids: {e}"))
            })?,
            None => StripShaLookup::empty(),
        };
        let mut last_blob_orig_sha: Option<Vec<u8>> = None;
        let mut blob_size_tracker = BlobSizeTracker::new(opts);
        let mut tracker = FilterTracker::new();
        let mut samples = ReportSamples::new();
        // Statistics counters
        let mut total_commits: usize = 0;
        let mut total_blobs: usize = 0;
        let mut path_compat_stats = PathCompatStats {
            policy: opts.path_compat_policy.as_str().to_string(),
            ..PathCompatStats::default()
        };
        let mut line = Vec::with_capacity(8192);
        // Track if the previous M-line used inline content; store commit_buf position and path bytes
        let mut pending_inline: Option<(usize, Vec<u8>)> = None;

        loop {
            line.clear();
            let read = fe_out.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }

            // Mirror original header/line when enabled
            if let Some(ref mut f) = orig_file_opt {
                f.write_all(&line)?;
            }

            // If swallowing a skipped annotated tag block, consume its lines and payload
            if skipping_tag_block {
                if line.starts_with(b"data ") {
                    let n = parse_data_size_header(&line)?;
                    let mut payload = vec![0u8; n];
                    fe_out.read_exact(&mut payload)?;
                    // Mirror original payload to debug file (when enabled)
                    if let Some(ref mut f) = orig_file_opt {
                        f.write_all(&payload)?;
                    }
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
                if let Some(last) = v.last() {
                    if *last == b'\n' {
                        v.pop();
                    }
                }
                for b in &mut v {
                    if *b >= b'A' && *b <= b'F' {
                        *b += 32;
                    }
                }
                last_blob_orig_sha = Some(v);
                continue;
            }

            // Blob begin
            if line == b"blob\n" {
                in_blob = true;
                blob_buf.clear();
                blob_buf.push(line.clone());
                last_blob_mark = None;
                total_blobs += 1;
                continue;
            }
            // Blob mark
            if in_blob && line.starts_with(b"mark :") {
                let mut num: u32 = 0;
                let mut seen = false;
                for &b in line[b"mark :".len()..].iter() {
                    if b.is_ascii_digit() {
                        seen = true;
                        num = num.saturating_mul(10).saturating_add((b - b'0') as u32);
                    } else {
                        break;
                    }
                }
                if seen {
                    last_blob_mark = Some(num);
                }
                blob_buf.push(line.clone());
                continue;
            }

            // If a lightweight tag reset is pending, capture its 'from ' line
            if crate::tag::maybe_capture_pending_tag_reset(
                &mut pending_tag_reset,
                &line,
                &mut buffered_tag_resets,
            ) {
                continue;
            }

            // Capture branch reset targets (reset refs/heads/<name> -> from ...)
            if let Some(ref_name) = pending_branch_reset.take() {
                if !in_commit && line.starts_with(b"from ") {
                    let mut target = line[b"from ".len()..].to_vec();
                    if let Some(last) = target.last() {
                        if *last == b'\n' {
                            target.pop();
                        }
                    }
                    if !target.is_empty() {
                        branch_reset_targets.push((ref_name, target));
                    }
                } else {
                    pending_branch_reset = Some(ref_name);
                }
            }

            // Buffer annotated tag blocks and emit once (rename/dedupe-safe)
            if line.starts_with(b"tag ") {
                let short_mapper = short_hash_mapper.as_ref();
                crate::tag::process_tag_block(
                    &line,
                    crate::tag::TagProcessContext {
                        fe_out: &mut fe_out,
                        orig_file: orig_file_opt.as_mut().map(|w| w as &mut dyn Write),
                        filt_file: &mut filt_file as &mut dyn Write,
                        fi_in: if let Some(ref mut fi_in) = fi_in_opt {
                            Some(fi_in as &mut dyn Write)
                        } else {
                            None
                        },
                        replacer: &replacer,
                        msg_regex: msg_regex_replacer.as_ref(),
                        short_mapper,
                        opts,
                        updated_refs: &mut updated_refs,
                        annotated_tag_refs: &mut annotated_tag_refs,
                        ref_renames: &mut ref_renames,
                        emitted_marks: &mut tracker.emitted_marks,
                    },
                )?;
                continue;
            }

            if line.starts_with(b"commit ") {
                // Start buffering a commit using possibly renamed header
                in_commit = true;
                commit_buf.clear();
                commit_has_changes = false;
                commit_mark = None;
                first_parent_mark = None;
                parent_lines.clear();
                total_commits += 1;
                let hdr = crate::commit::rename_commit_header_ref(&line, opts, &mut ref_renames);
                commit_buf.extend_from_slice(&hdr);
                // Track final branch ref (post-rename) for HEAD updates
                let mut refname = &hdr[b"commit ".len()..];
                if let Some(&last) = refname.last() {
                    if last == b'\n' {
                        refname = &refname[..refname.len() - 1];
                    }
                }
                if refname.starts_with(b"refs/heads/") {
                    updated_branch_refs.insert(refname.to_vec());
                }
                continue;
            }

            // If we are buffering a commit, process its content
            if in_commit {
                // End of commit is implicit: a new object starts.
                if line.starts_with(b"commit ")
                    || line.starts_with(b"tag ")
                    || line.starts_with(b"reset ")
                    || line.starts_with(b"blob")
                    || line == b"done\n"
                {
                    let short_mapper = short_hash_mapper.as_ref();
                    let mut path_events = Vec::new();
                    match crate::commit::process_commit_line(
                        b"\n",
                        opts,
                        &mut fe_out,
                        orig_file_opt.as_mut().map(|w| w as &mut dyn Write),
                        &mut filt_file as &mut dyn Write,
                        if let Some(ref mut fi_in) = fi_in_opt {
                            Some(fi_in as &mut dyn Write)
                        } else {
                            None
                        },
                        &replacer,
                        msg_regex_replacer.as_ref(),
                        short_mapper,
                        &mut commit_buf,
                        &mut commit_has_changes,
                        &mut commit_mark,
                        &mut first_parent_mark,
                        &mut commit_original_oid,
                        &mut parent_count,
                        &mut commit_pairs,
                        &mut import_broken,
                        &mut parent_lines,
                        &mut alias_map,
                        &tracker.emitted_marks,
                        &mut path_events,
                    )? {
                        crate::commit::CommitAction::Consumed => {} // Should not happen with synthetic newline
                        crate::commit::CommitAction::Ended => {
                            // Record emitted commit mark
                            if let Some(m) = commit_mark {
                                tracker.emitted_marks.insert(m);
                                if let (Some(mapper), Some(ref mut fi_in), Some(ref mut fi_out)) = (
                                    short_hash_mapper.as_mut(),
                                    fi_in_opt.as_mut(),
                                    fi_out_opt.as_mut(),
                                ) {
                                    if let Some((old, Some(mark))) = commit_pairs.last() {
                                        if *mark == m {
                                            if let Some(new_id) =
                                                resolve_mark_oid(fi_in, fi_out, *mark)?
                                            {
                                                mapper.update_mapping(old, &new_id);
                                            }
                                        }
                                    }
                                }
                            }
                            in_commit = false;
                        }
                    }
                    for event in path_events {
                        record_path_compat_event(&mut path_compat_stats, event);
                    }
                }
            }
            if in_commit {
                let mut pending_inline_ctx = PendingInlineDataCtx {
                    opts,
                    fe_out: &mut fe_out,
                    orig_file_opt: &mut orig_file_opt,
                    commit_buf: &mut commit_buf,
                    commit_has_changes: &mut commit_has_changes,
                    pending_inline: &mut pending_inline,
                    samples: &mut samples,
                    path_compat_stats: &mut path_compat_stats,
                    content_replacer: &content_replacer,
                    content_regex_replacer: &content_regex_replacer,
                };
                if process_pending_inline_data_line(&line, &mut pending_inline_ctx)? {
                    continue;
                }
                if line.starts_with(b"M ") && {
                    let mut ctx = CommitMPrecheckCtx {
                        opts,
                        commit_buf: &mut commit_buf,
                        commit_has_changes: &mut commit_has_changes,
                        pending_inline: &mut pending_inline,
                        tracker: &mut tracker,
                        samples: &mut samples,
                        path_compat_stats: &mut path_compat_stats,
                        strip_sha_lookup: &strip_sha_lookup,
                        blob_size_tracker: &mut blob_size_tracker,
                    };
                    process_commit_m_line_precheck(&line, &mut ctx)?
                } {
                    continue;
                }

                let processed_line = if in_commit {
                    rewrite_commit_identity_line(
                        &line,
                        opts,
                        author_rewriter.as_ref(),
                        committer_rewriter.as_ref(),
                        email_rewriter.as_ref(),
                        mailmap_rewriter.as_ref(),
                    )
                } else {
                    line.clone()
                };

                let short_mapper = short_hash_mapper.as_ref();
                let mut path_events = Vec::new();
                match crate::commit::process_commit_line(
                    &processed_line,
                    opts,
                    &mut fe_out,
                    orig_file_opt.as_mut().map(|w| w as &mut dyn Write),
                    &mut filt_file as &mut dyn Write,
                    if let Some(ref mut fi_in) = fi_in_opt {
                        Some(fi_in)
                    } else {
                        None
                    },
                    &replacer,
                    msg_regex_replacer.as_ref(),
                    short_mapper,
                    &mut commit_buf,
                    &mut commit_has_changes,
                    &mut commit_mark,
                    &mut first_parent_mark,
                    &mut commit_original_oid,
                    &mut parent_count,
                    &mut commit_pairs,
                    &mut import_broken,
                    &mut parent_lines,
                    &mut alias_map,
                    &tracker.emitted_marks,
                    &mut path_events,
                )? {
                    crate::commit::CommitAction::Consumed => {
                        for event in path_events {
                            record_path_compat_event(&mut path_compat_stats, event);
                        }
                        continue;
                    }
                    crate::commit::CommitAction::Ended => {
                        for event in path_events {
                            record_path_compat_event(&mut path_compat_stats, event);
                        }
                        if let Some(m) = commit_mark {
                            tracker.emitted_marks.insert(m);
                            if let (Some(mapper), Some(ref mut fi_in), Some(ref mut fi_out)) = (
                                short_hash_mapper.as_mut(),
                                fi_in_opt.as_mut(),
                                fi_out_opt.as_mut(),
                            ) {
                                if let Some((old, Some(mark))) = commit_pairs.last() {
                                    if *mark == m {
                                        if let Some(new_id) =
                                            resolve_mark_oid(fi_in, fi_out, *mark)?
                                        {
                                            mapper.update_mapping(old, &new_id);
                                        }
                                    }
                                }
                            }
                        }
                        in_commit = false;
                    }
                }
            }

            // Generic data blocks (e.g., blob): forward exact payload bytes
            if line.starts_with(b"data ") {
                let n = parse_data_size_header(&line)?;
                let mut payload = vec![0u8; n];
                fe_out.read_exact(&mut payload)?;
                // Always mirror to original (when enabled)
                if let Some(ref mut f) = orig_file_opt {
                    f.write_all(&payload)?;
                }
                if in_blob {
                    let mut ctx = BlobPayloadCtx {
                        opts,
                        filt_file: &mut filt_file,
                        fi_in_opt: &mut fi_in_opt,
                        content_replacer: &content_replacer,
                        content_regex_replacer: &content_regex_replacer,
                        in_blob: &mut in_blob,
                        blob_buf: &mut blob_buf,
                        last_blob_mark: &mut last_blob_mark,
                        last_blob_orig_sha: &mut last_blob_orig_sha,
                        tracker: &mut tracker,
                        import_broken: &mut import_broken,
                        strip_sha_lookup: &strip_sha_lookup,
                    };
                    process_blob_data_payload(payload, &mut ctx)?;
                    continue;
                } else {
                    // Not a blob payload (should be rare here); forward as-is
                    filt_file.write_all(&line)?;
                    if let Some(ref mut fi_in) = fi_in_opt {
                        if let Err(e) = fi_in.write_all(&line) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                import_broken = true;
                                break;
                            } else {
                                return Err(e.into());
                            }
                        }
                    }
                    filt_file.write_all(&payload)?;
                    if let Some(ref mut fi_in) = fi_in_opt {
                        if let Err(e) = fi_in.write_all(&payload) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                import_broken = true;
                                break;
                            } else {
                                return Err(e.into());
                            }
                        }
                    }
                    continue;
                }
                // Do not consume or inject an extra newline; next header line follows in stream.
            }

            // Handle end-of-stream marker; flush buffered lightweight tag resets before 'done'
            if line == b"done\n" {
                crate::finalize::flush_lightweight_tag_resets(
                    &mut buffered_tag_resets,
                    &annotated_tag_refs,
                    &mut filt_file as &mut dyn Write,
                    fi_in_opt.as_mut().map(|w| w as &mut dyn Write),
                    &mut import_broken,
                )?;
                // Forward 'done' after flushing
                filt_file.write_all(&line)?;
                if let Some(ref mut fi_in) = fi_in_opt {
                    if let Err(e) = fi_in.write_all(&line) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            import_broken = true;
                            break;
                        } else {
                            return Err(e.into());
                        }
                    }
                }
                continue;
            }

            // Lightweight tag renames: reset refs/tags/<name>
            if crate::tag::process_reset_header(
                &line,
                opts,
                &mut ref_renames,
                &mut pending_tag_reset,
            ) {
                continue;
            }

            // Branch reset renames: reset refs/heads/<name>
            if line.starts_with(b"reset ") {
                let mut name = &line[b"reset ".len()..];
                if let Some(&last) = name.last() {
                    if last == b'\n' {
                        name = &name[..name.len() - 1];
                    }
                }
                if name.starts_with(b"refs/heads/") {
                    let mut out = line.clone();
                    let mut final_ref = name.to_vec();
                    if let Some((ref old, ref new_)) = opts.branch_rename {
                        let bname = &name[b"refs/heads/".len()..];
                        if bname.starts_with(&old[..]) {
                            let mut rebuilt = Vec::with_capacity(
                                7 + b"refs/heads/".len()
                                    + new_.len()
                                    + (bname.len() - old.len())
                                    + 1,
                            );
                            rebuilt.extend_from_slice(b"reset ");
                            rebuilt.extend_from_slice(b"refs/heads/");
                            rebuilt.extend_from_slice(new_);
                            rebuilt.extend_from_slice(&bname[old.len()..]);
                            rebuilt.push(b'\n');
                            let new_full =
                                [b"refs/heads/".as_ref(), new_, &bname[old.len()..]].concat();
                            ref_renames.insert((name.to_vec(), new_full.clone()));
                            final_ref = new_full;
                            out = rebuilt;
                        }
                    }
                    updated_branch_refs.insert(final_ref.clone());
                    pending_branch_reset = Some(final_ref);
                    // forward
                    filt_file.write_all(&out)?;
                    if let Some(ref mut fi_in) = fi_in_opt {
                        if let Err(e) = fi_in.write_all(&out) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                import_broken = true;
                            } else {
                                return Err(e.into());
                            }
                        }
                    }
                    continue;
                }
            }

            // Forward non-message lines as-is to filtered + import (drop stray blanks)
            if line == b"\n" {
                continue;
            }
            filt_file.write_all(&line)?;
            if let Some(ref mut fi_in) = fi_in_opt {
                if let Err(e) = fi_in.write_all(&line) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        import_broken = true;
                        break;
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }

        drop(fi_out_opt);
        self.finalize_stream(
            opts,
            &mut filt_file,
            &mut orig_file_opt,
            &mut fi_in_opt,
            &mut fe,
            &mut fi,
            import_broken,
            ref_renames,
            commit_pairs,
            buffered_tag_resets,
            annotated_tag_refs,
            updated_branch_refs,
            branch_reset_targets,
            tracker,
            samples,
            total_commits,
            total_blobs,
            path_compat_stats,
            &blob_size_tracker,
        )?;

        // Wait for child processes to finish
        let _ = fe.wait()?;
        if let Some(mut child) = fi {
            let _ = child.wait()?;
        }

        Ok(())
    }
}

pub fn run(opts: &Options) -> FilterRepoResult<()> {
    StreamProcessor::new(opts)?.process()
}

fn resolve_mark_oid(
    fi_in: &mut dyn Write,
    fi_out: &mut BufReader<std::process::ChildStdout>,
    mark: u32,
) -> io::Result<Option<Vec<u8>>> {
    let cmd = format!("get-mark :{}\n", mark);
    fi_in.write_all(cmd.as_bytes())?;
    fi_in.flush()?;
    let mut line = Vec::with_capacity(64);
    loop {
        line.clear();
        let read = fi_out.read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let mut end = line.len();
        while end > 0 && (line[end - 1] == b'\n' || line[end - 1] == b'\r') {
            end -= 1;
        }
        if end == 0 {
            continue;
        }
        let trimmed = &line[..end];
        let slice = if let Some(rest) = trimmed.strip_prefix(b"mark ") {
            let mut idx = 0usize;
            let mut value: u32 = 0;
            let mut seen_digit = false;
            while idx < rest.len() {
                let b = rest[idx];
                if b.is_ascii_digit() {
                    seen_digit = true;
                    value = value.saturating_mul(10).saturating_add((b - b'0') as u32);
                    idx += 1;
                } else {
                    break;
                }
            }
            if !seen_digit || value != mark {
                continue;
            }
            while idx < rest.len() && rest[idx] == b' ' {
                idx += 1;
            }
            &rest[idx..]
        } else {
            trimmed
        };
        if !slice.iter().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let mut oid = slice.to_vec();
        for byte in &mut oid {
            if (b'A'..=b'F').contains(byte) {
                *byte += 32;
            }
        }
        return Ok(Some(oid));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_opts(source: &str) -> Options {
        let mut opts = Options::default();
        opts.source = PathBuf::from(source);
        opts.target = PathBuf::from(".");
        opts.refs = vec!["--all".to_string()];
        opts.cleanup = crate::opts::CleanupMode::None;
        opts.enforce_sanity = false;
        opts
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
    fn test_blob_size_tracker_batch_mode_query() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        std::process::Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .unwrap();

        for (key, value) in [
            ("user.name", "Blob Size Tester"),
            ("user.email", "blob-size@example.com"),
        ] {
            let status = std::process::Command::new("git")
                .args(["-C", repo_path.to_str().unwrap(), "config", key, value])
                .status()
                .expect("failed to configure git test repo");
            assert!(status.success(), "failed to set git config {key}");
        }

        let large_path = repo_path.join("large2.bin");
        std::fs::write(&large_path, vec![b'a'; 8192]).unwrap();
        let status = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .status()
            .expect("failed to add files to git test repo");
        assert!(status.success(), "git add failed for test repo");
        let status = std::process::Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "commit",
                "-m",
                "add files",
            ])
            .status()
            .expect("failed to commit files in git test repo");
        assert!(status.success(), "git commit failed for test repo");

        let ls_tree = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "ls-tree", "-r", "HEAD"])
            .output()
            .unwrap();
        let listing = String::from_utf8(ls_tree.stdout).unwrap();
        let mut large_sha = None;
        for line in listing.lines() {
            if let Some((meta, _path)) = line.split_once('\t') {
                let mut parts = meta.split_whitespace();
                let _mode = parts.next();
                let kind = parts.next();
                let sha = parts.next();
                if kind == Some("blob") {
                    large_sha = sha.map(|s| s.as_bytes().to_vec());
                }
            }
        }
        let large_sha = large_sha.expect("blob sha");

        let mut opts = create_test_opts(repo_path.to_str().unwrap());
        opts.max_blob_size = Some(1024);
        let mut tracker = BlobSizeTracker::new(&opts);
        // Force the fallback path (pretend prefetch did not run)
        tracker.prefetch_ok = false;
        assert!(tracker.is_oversize(&large_sha));
    }

    #[test]
    fn test_blob_size_tracker_detects_large_blob() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        std::process::Command::new("git")
            .args(["init", repo_path.to_str().unwrap()])
            .output()
            .unwrap();

        for (key, value) in [
            ("user.name", "Blob Size Tester"),
            ("user.email", "blob-size@example.com"),
        ] {
            let status = std::process::Command::new("git")
                .args(["-C", repo_path.to_str().unwrap(), "config", key, value])
                .status()
                .expect("failed to configure git test repo");
            assert!(status.success(), "failed to set git config {key}");
        }

        let large_path = repo_path.join("large.bin");
        let small_path = repo_path.join("small.txt");
        std::fs::write(&large_path, vec![b'a'; 4096]).unwrap();
        std::fs::write(&small_path, b"hello").unwrap();

        let status = std::process::Command::new("git")
            .args(["-C", repo_path.to_str().unwrap(), "add", "."])
            .status()
            .expect("failed to add files to git test repo");
        assert!(status.success(), "git add failed for test repo");

        let status = std::process::Command::new("git")
            .args([
                "-C",
                repo_path.to_str().unwrap(),
                "commit",
                "-m",
                "add files",
            ])
            .status()
            .expect("failed to commit files in git test repo");
        assert!(status.success(), "git commit failed for test repo");

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
