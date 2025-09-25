use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, CellAlignment,
    ContentArrangement, Table,
};
use serde::Serialize;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::gitutil;
use crate::opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};
use crate::pathutil::dequote_c_style_bytes;
use crate::pipes;

// Simple footnote registry to keep human output compact by moving 40-char OIDs
// to a dedicated footnotes list printed at the bottom.
#[derive(Default)]
struct FootnoteRegistry {
    map: HashMap<String, usize>,
    entries: Vec<(usize, String, Option<String>)>, // (index, oid, context)
}

impl FootnoteRegistry {
    fn new() -> Self {
        Self::default()
    }

    // Register an OID with optional context (e.g., example path) and return "[n]" marker.
    fn note(&mut self, oid: &str, context: Option<&str>) -> String {
        if let Some(&idx) = self.map.get(oid) {
            return format!("[{}]", idx);
        }
        let idx = self.entries.len() + 1;
        self.map.insert(oid.to_string(), idx);
        // Keep the first non-empty context we see
        self.entries.push((
            idx,
            oid.to_string(),
            context.filter(|s| !s.is_empty()).map(|s| s.to_string()),
        ));
        format!("[{}]", idx)
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WarningLevel {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct Warning {
    pub level: WarningLevel,
    pub message: String,
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ObjectStat {
    pub oid: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DirectoryStat {
    pub path: String,
    pub entries: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PathStat {
    pub path: String,
    pub length: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DuplicateBlobStat {
    pub oid: String,
    pub paths: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CommitMessageStat {
    pub oid: String,
    pub length: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RepositoryMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    pub loose_objects: u64,
    pub loose_size_bytes: u64,
    pub packed_objects: u64,
    pub packed_size_bytes: u64,
    pub total_objects: u64,
    pub total_size_bytes: u64,
    pub object_types: BTreeMap<String, u64>,
    pub tree_total_size_bytes: u64,
    pub refs_total: usize,
    pub refs_heads: usize,
    pub refs_tags: usize,
    pub refs_remotes: usize,
    pub refs_other: usize,
    pub largest_blobs: Vec<ObjectStat>,
    pub largest_trees: Vec<ObjectStat>,
    pub blobs_over_threshold: Vec<ObjectStat>,
    pub directory_hotspots: Option<DirectoryStat>,
    pub longest_path: Option<PathStat>,
    pub duplicate_blobs: Vec<DuplicateBlobStat>,
    pub max_commit_parents: usize,
    pub oversized_commit_messages: Vec<CommitMessageStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisReport {
    pub metrics: RepositoryMetrics,
    pub warnings: Vec<Warning>,
}

pub fn run(opts: &Options) -> io::Result<()> {
    debug_assert_eq!(opts.mode, Mode::Analyze);
    let report = generate_report(opts)?;
    if opts.analyze.json {
        let json = serde_json::to_string_pretty(&report).map_err(to_io_error)?;
        println!("{}", json);
    } else {
        print_human(&report, &opts.analyze);
    }
    Ok(())
}

pub fn generate_report(opts: &Options) -> io::Result<AnalysisReport> {
    // Avoid Windows verbatim (\\?\) paths which can confuse external tools like Git when
    // passed via command-line flags. Use the provided path directly.
    let repo = opts.source.clone();
    let metrics = collect_metrics(&repo, &opts.analyze)?;
    let warnings = evaluate_warnings(&metrics, &opts.analyze.thresholds);
    Ok(AnalysisReport { metrics, warnings })
}

fn collect_metrics(repo: &Path, cfg: &AnalyzeConfig) -> io::Result<RepositoryMetrics> {
    let mut metrics = RepositoryMetrics::default();
    metrics.workdir = Some(repo.display().to_string());
    gather_footprint(repo, &mut metrics)?;
    gather_refs(repo, &mut metrics)?;
    // History-wide scan via fast-export for reachable blobs/commits and path mapping
    gather_history_fast_export(repo, cfg, &mut metrics)?;
    // Tree inventory via cat-file for counts and top sizes (best-effort)
    gather_tree_inventory(repo, cfg, &mut metrics)?;
    // Keep a quick HEAD snapshot for context
    gather_worktree_snapshot(repo, cfg, &mut metrics)?;
    Ok(metrics)
}

fn gather_footprint(repo: &Path, metrics: &mut RepositoryMetrics) -> io::Result<()> {
    let output = run_git_capture(repo, &["count-objects", "-v"])?;
    for line in output.lines() {
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        match key {
            "count" => metrics.loose_objects = value.parse::<u64>().unwrap_or(0),
            "size" => metrics.loose_size_bytes = value.parse::<u64>().unwrap_or(0) * 1024,
            "in-pack" => metrics.packed_objects = value.parse::<u64>().unwrap_or(0),
            "size-pack" => metrics.packed_size_bytes = value.parse::<u64>().unwrap_or(0) * 1024,
            _ => {}
        }
    }
    metrics.total_objects = metrics.loose_objects + metrics.packed_objects;
    metrics.total_size_bytes = metrics.loose_size_bytes + metrics.packed_size_bytes;
    Ok(())
}

fn gather_tree_inventory(
    repo: &Path,
    cfg: &AnalyzeConfig,
    metrics: &mut RepositoryMetrics,
) -> io::Result<()> {
    let mut largest_trees: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
    let mut tree_count: u64 = 0;
    let mut tree_total: u64 = 0;
    let mut child = Command::new("git")
        .current_dir(repo)
        .arg("cat-file")
        .arg("--batch-check")
        .arg("--batch-all-objects")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "failed to capture git cat-file stdout",
        )
    })?;
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line?;
        let mut parts = line.split_whitespace();
        let oid = parts.next().unwrap_or("");
        let typ = parts.next().unwrap_or("");
        let size = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        if typ == "tree" {
            tree_count += 1;
            tree_total = tree_total.saturating_add(size);
            push_top(&mut largest_trees, cfg.top, size, oid);
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "git cat-file --batch-check failed",
        ));
    }
    if tree_count > 0 {
        metrics.object_types.insert("tree".to_string(), tree_count);
    }
    metrics.tree_total_size_bytes = tree_total;
    metrics.largest_trees = heap_to_vec(largest_trees);
    Ok(())
}

fn gather_refs(repo: &Path, metrics: &mut RepositoryMetrics) -> io::Result<()> {
    let refs = gitutil::get_all_refs(repo)?;
    for name in refs.keys() {
        let name = name.as_str();
        metrics.refs_total += 1;
        if name.starts_with("refs/heads/") {
            metrics.refs_heads += 1;
        } else if name.starts_with("refs/tags/") {
            metrics.refs_tags += 1;
        } else if name.starts_with("refs/remotes/") {
            metrics.refs_remotes += 1;
        } else {
            metrics.refs_other += 1;
        }
    }
    Ok(())
}

fn gather_worktree_snapshot(
    repo: &Path,
    cfg: &AnalyzeConfig,
    metrics: &mut RepositoryMetrics,
) -> io::Result<()> {
    let head = run_git_capture(repo, &["rev-parse", "--verify", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if head.is_empty() {
        return Ok(());
    }
    let mut child = Command::new("git")
        .current_dir(repo)
        .arg("ls-tree")
        .arg("-r")
        .arg("--full-tree")
        .arg("-z")
        .arg(&head)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "failed to capture git ls-tree stdout")
    })?;
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let mut directories: HashMap<String, usize> = HashMap::new();
    let mut duplicates: HashMap<String, DuplicateBlobStat> = HashMap::new();
    let mut sample_paths: HashMap<String, String> = HashMap::new();
    while read_until(&mut reader, 0, &mut buf)? {
        if buf.is_empty() {
            continue;
        }
        let entry = String::from_utf8_lossy(&buf[..buf.len() - 1]);
        let mut parts = entry.splitn(2, '\t');
        let meta = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");
        let mut meta_parts = meta.split_whitespace();
        let _mode = meta_parts.next();
        let typ = meta_parts.next().unwrap_or("");
        let oid = meta_parts.next().unwrap_or("");
        if typ == "blob" {
            let len = path.len();
            if let Some(current) = &mut metrics.longest_path {
                if len > current.length {
                    *current = PathStat {
                        path: path.to_string(),
                        length: len,
                    };
                }
            } else {
                metrics.longest_path = Some(PathStat {
                    path: path.to_string(),
                    length: len,
                });
            }
            sample_paths
                .entry(oid.to_string())
                .or_insert_with(|| path.to_string());
            let entry = duplicates
                .entry(oid.to_string())
                .or_insert_with(|| DuplicateBlobStat {
                    oid: oid.to_string(),
                    paths: 0,
                    example_path: Some(path.to_string()),
                });
            entry.paths += 1;
        }
        if let Some(dir) = parent_directory(path) {
            *directories.entry(dir).or_insert(0) += 1;
        } else {
            *directories.entry(String::from(".")).or_insert(0) += 1;
        }
        buf.clear();
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::new(io::ErrorKind::Other, "git ls-tree failed"));
    }
    // If some of the top blobs are not present in HEAD, look up a historical path
    // from any revision in the repository history to improve readability.
    let mut needed: HashSet<String> = HashSet::new();
    for b in metrics.largest_blobs.iter().chain(&metrics.blobs_over_threshold) {
        if !sample_paths.contains_key(&b.oid) {
            needed.insert(b.oid.clone());
        }
    }
    let mut history_paths: HashMap<String, String> = HashMap::new();
    if !needed.is_empty() {
        history_paths = map_oids_to_paths_from_history(repo, &needed)?;
    }
    let mut duplicates_vec: Vec<DuplicateBlobStat> = duplicates
        .into_iter()
        .filter(|(_, stat)| stat.paths > 1)
        .map(|(_, stat)| stat)
        .collect();
    duplicates_vec.sort_by(|a, b| b.paths.cmp(&a.paths));
    duplicates_vec.truncate(cfg.top);
    metrics.duplicate_blobs = duplicates_vec;
    for blob in &mut metrics.largest_blobs {
        if let Some(path) = sample_paths.get(&blob.oid) {
            blob.path = Some(path.clone());
        } else if let Some(path) = history_paths.get(&blob.oid) {
            blob.path = Some(path.clone());
        }
    }
    for blob in &mut metrics.blobs_over_threshold {
        if let Some(path) = sample_paths.get(&blob.oid) {
            blob.path = Some(path.clone());
        } else if let Some(path) = history_paths.get(&blob.oid) {
            blob.path = Some(path.clone());
        }
    }
    if let Some((path, entries)) = directories.into_iter().max_by_key(|(_, count)| *count) {
        metrics.directory_hotspots = Some(DirectoryStat { path, entries });
    }
    Ok(())
}

// Map a set of blob OIDs to one example path from repository history (any revision).
// Uses `git rev-list --objects --all -z` to safely parse paths with spaces.
fn map_oids_to_paths_from_history(
    repo: &Path,
    needed: &HashSet<String>,
) -> io::Result<HashMap<String, String>> {
    let mut found: HashMap<String, String> = HashMap::new();
    let mut child = Command::new("git")
        .current_dir(repo)
        .arg("rev-list")
        .arg("--objects")
        .arg("--all")
        .arg("-z")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "failed to capture git rev-list stdout",
        )
    })?;
    let mut reader = BufReader::new(stdout);
    let mut rec = Vec::new();
    while read_until(&mut reader, 0, &mut rec)? {
        if rec.is_empty() {
            continue;
        }
        // Record is "<oid> <path>" or just "<oid>" (for commits or objects without path)
        let line = String::from_utf8_lossy(&rec[..rec.len() - 1]);
        // Split once on whitespace
        let mut it = line.splitn(2, char::is_whitespace);
        let oid = it.next().unwrap_or("");
        if needed.contains(oid) {
            if let Some(path) = it.next() {
                if !path.is_empty() && !found.contains_key(oid) {
                    found.insert(oid.to_string(), path.to_string());
                    if found.len() >= needed.len() {
                        break;
                    }
                }
            }
        }
        rec.clear();
    }
    drop(reader);
    let _ = child.wait();
    Ok(found)
}

// History-wide metrics via fast-export: blob/path mapping, sizes for top N, commit parents,
// oversized commit messages. This intentionally ignores blob payloads.
fn gather_history_fast_export(
    repo: &Path,
    cfg: &AnalyzeConfig,
    metrics: &mut RepositoryMetrics,
) -> io::Result<()> {
    let mut fe_opts = Options::default();
    fe_opts.source = repo.to_path_buf();
    fe_opts.no_data = true;
    fe_opts.quotepath = true;
    let mut cmd = pipes::build_fast_export_cmd(&fe_opts)?;
    let mut child = cmd.stdout(Stdio::piped()).spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "failed to capture git fast-export stdout",
        )
    })?;
    let mut reader = BufReader::new(stdout);

    let mut line = Vec::new();
    let mut in_commit = false;
    let mut cur_commit_oid: Option<String> = None;
    let mut cur_parents: usize = 0;
    let mut commit_count: u64 = 0;

    let mut blob_paths: HashMap<String, HashSet<String>> = HashMap::new();
    let mut blob_example_path: HashMap<String, String> = HashMap::new();

    while reader.read_until(b'\n', &mut line)? != 0 {
        if line.starts_with(b"commit ") {
            in_commit = true;
            cur_commit_oid = None;
            cur_parents = 0;
            commit_count = commit_count.saturating_add(1);
            line.clear();
            continue;
        }
        if in_commit {
            if line == b"\n" {
                if cur_parents > metrics.max_commit_parents {
                    metrics.max_commit_parents = cur_parents;
                }
                in_commit = false;
                line.clear();
                continue;
            }
            if line.starts_with(b"original-oid ") {
                let s = String::from_utf8_lossy(&line[b"original-oid ".len()..])
                    .trim()
                    .to_string();
                cur_commit_oid = Some(s);
                line.clear();
                continue;
            }
            if line.starts_with(b"from ") || line.starts_with(b"merge ") {
                cur_parents = cur_parents.saturating_add(1);
                line.clear();
                continue;
            }
            if line.starts_with(b"data ") {
                let n = parse_size_after_data(&line)?;
                if n > cfg.thresholds.warn_commit_msg_bytes {
                    if let Some(oid) = cur_commit_oid.clone() {
                        metrics
                            .oversized_commit_messages
                            .push(CommitMessageStat { oid, length: n });
                    }
                }
                // Read and discard payload
                let mut payload = vec![0u8; n];
                reader.read_exact(&mut payload)?;
                line.clear();
                continue;
            }
            if line.starts_with(b"M ") {
                if let Some((oid, path)) = parse_modify_line(&line) {
                    if oid.len() == 40 && oid.chars().all(|c| c.is_ascii_hexdigit()) {
                        let oid_lower = oid.to_ascii_lowercase();
                        blob_paths
                            .entry(oid_lower.clone())
                            .or_default()
                            .insert(path.clone());
                        blob_example_path.entry(oid_lower).or_insert(path);
                    }
                }
                line.clear();
                continue;
            }
        }
        line.clear();
    }
    let _ = child.wait();

    // Summarize object type counts from what we observed
    metrics
        .object_types
        .insert("commit".to_string(), commit_count);
    metrics
        .object_types
        .insert("blob".to_string(), blob_paths.len() as u64);

    // Fetch sizes for all observed blobs, then compute top lists
    let sizes = batch_check_blob_sizes(repo, blob_paths.keys())?;
    let mut largest_blobs: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
    let mut threshold_hits: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
    for (oid, size) in &sizes {
        push_top(&mut largest_blobs, cfg.top, *size, oid);
        if *size >= cfg.thresholds.warn_blob_bytes {
            push_top(&mut threshold_hits, cfg.top, *size, oid);
        }
    }
    metrics.largest_blobs = heap_to_vec(largest_blobs);
    for blob in &mut metrics.largest_blobs {
        if let Some(path) = blob_example_path.get(&blob.oid) {
            blob.path = Some(path.clone());
        }
    }
    metrics.blobs_over_threshold = heap_to_vec(threshold_hits);
    for blob in &mut metrics.blobs_over_threshold {
        if let Some(path) = blob_example_path.get(&blob.oid) {
            blob.path = Some(path.clone());
        }
    }

    // Duplicate blobs across history: rank by unique path count
    let mut dups: Vec<DuplicateBlobStat> = blob_paths
        .into_iter()
        .filter_map(|(oid, paths)| {
            let count = paths.len();
            if count > 1 {
                Some(DuplicateBlobStat {
                    oid,
                    paths: count,
                    example_path: None,
                })
            } else {
                None
            }
        })
        .collect();
    dups.sort_by(|a, b| b.paths.cmp(&a.paths));
    dups.truncate(cfg.top);
    metrics.duplicate_blobs = dups;

    Ok(())
}

fn parse_size_after_data(line: &[u8]) -> io::Result<usize> {
    if !line.starts_with(b"data ") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected data header",
        ));
    }
    let size_bytes = &line[b"data ".len()..];
    let n = std::str::from_utf8(size_bytes)
        .ok()
        .map(|s| s.trim())
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid data header"))?;
    Ok(n)
}

// Parse an 'M <mode> <id> <path>' fast-export line; return (id, decoded_path)
fn parse_modify_line(line: &[u8]) -> Option<(String, String)> {
    if !line.starts_with(b"M ") {
        return None;
    }
    let rest = &line[2..];
    let space1 = rest.iter().position(|&b| b == b' ')?;
    let rest = &rest[space1 + 1..]; // after mode
    let space2 = rest.iter().position(|&b| b == b' ')?;
    let id = String::from_utf8_lossy(&rest[..space2]).to_string();
    let path_part = &rest[space2 + 1..];
    // Decode path with C-style quoting if needed, strip trailing newline
    let decoded = if !path_part.is_empty() && path_part[0] == b'"' {
        // find closing quote respecting escapes
        let mut idx = 1usize;
        let mut found = None;
        while idx < path_part.len() {
            if path_part[idx] == b'"' {
                // count preceding backslashes
                let mut backslashes = 0usize;
                let mut j = idx;
                while j > 0 && path_part[j - 1] == b'\\' {
                    backslashes += 1;
                    j -= 1;
                }
                if backslashes % 2 == 0 {
                    found = Some(idx);
                    break;
                }
            }
            idx += 1;
        }
        let end = found?;
        let bytes = dequote_c_style_bytes(&path_part[1..end]);
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        let s = String::from_utf8_lossy(path_part).to_string();
        s.trim().to_string()
    };
    Some((id, decoded))
}

fn batch_check_blob_sizes<'a, I>(repo: &Path, oids: I) -> io::Result<HashMap<String, u64>>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut child = Command::new("git")
        .current_dir(repo)
        .arg("cat-file")
        .arg("--batch-check")
        .stdout(Stdio::piped())
        .stdin(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    // Feed all OIDs
    for oid in oids {
        stdin.write_all(oid.as_bytes())?;
        stdin.write_all(b"\n")?;
    }
    drop(stdin);
    let mut sizes: HashMap<String, u64> = HashMap::new();
    let mut line = String::new();
    while {
        line.clear();
        stdout.read_line(&mut line)? != 0
    } {
        // Format: "<oid> <type> <size>"
        let mut it = line.split_whitespace();
        let oid = match it.next() {
            Some(s) => s,
            None => continue,
        };
        let typ = match it.next() {
            Some(s) => s,
            None => continue,
        };
        let sz = match it.next() {
            Some(s) => s.parse::<u64>().ok(),
            None => None,
        };
        if typ == "blob" {
            if let Some(n) = sz {
                sizes.insert(oid.to_string(), n);
            }
        }
    }
    let _ = child.wait();
    Ok(sizes)
}

// (removed old gather_history_stats; superseded by gather_history_fast_export)

fn evaluate_warnings(metrics: &RepositoryMetrics, thresholds: &AnalyzeThresholds) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if metrics.total_size_bytes >= thresholds.crit_total_bytes {
        warnings.push(Warning {
      level: WarningLevel::Critical,
      message: format!(
        "Repository is {:.2} GiB (threshold {:.2} GiB).", to_gib(metrics.total_size_bytes), to_gib(thresholds.crit_total_bytes)
      ),
      recommendation: Some("Avoid storing generated files or large media in Git; consider Git-LFS or external storage.".to_string()),
    });
    } else if metrics.total_size_bytes >= thresholds.warn_total_bytes {
        warnings.push(Warning {
            level: WarningLevel::Warning,
            message: format!(
                "Repository is {:.2} GiB (warning threshold {:.2} GiB).",
                to_gib(metrics.total_size_bytes),
                to_gib(thresholds.warn_total_bytes)
            ),
            recommendation: Some(
                "Prune large assets or split the project to keep Git operations fast.".to_string(),
            ),
        });
    }
    if metrics.refs_total >= thresholds.warn_ref_count {
        warnings.push(Warning {
            level: WarningLevel::Warning,
            message: format!(
                "Repository has {} refs (warning threshold {}).",
                metrics.refs_total, thresholds.warn_ref_count
            ),
            recommendation: Some(
                "Delete stale branches/tags or move rarely-needed refs to a separate remote."
                    .to_string(),
            ),
        });
    }
    if metrics.total_objects as usize >= thresholds.warn_object_count {
        warnings.push(Warning {
      level: WarningLevel::Warning,
      message: format!(
        "Repository contains {} Git objects (warning threshold {}).",
        metrics.total_objects,
        thresholds.warn_object_count
      ),
      recommendation: Some("Consider sharding the project or aggregating many tiny files to reduce object churn.".to_string()),
    });
    }
    if let Some(dir) = &metrics.directory_hotspots {
        if dir.entries >= thresholds.warn_tree_entries {
            warnings.push(Warning {
        level: WarningLevel::Warning,
        message: format!(
          "Directory '{}' has {} entries (threshold {}).", dir.path, dir.entries, thresholds.warn_tree_entries
        ),
        recommendation: Some("Shard large directories into smaller subdirectories to keep tree traversals fast.".to_string()),
      });
        }
    }
    if let Some(path) = &metrics.longest_path {
        if path.length >= thresholds.warn_path_length {
            warnings.push(Warning {
        level: WarningLevel::Warning,
        message: format!(
          "Path '{}' is {} characters long (threshold {}).", path.path, path.length, thresholds.warn_path_length
        ),
        recommendation: Some("Shorten deeply nested names to improve compatibility with tooling and filesystems.".to_string()),
      });
        }
    }
    for blob in &metrics.blobs_over_threshold {
        warnings.push(Warning {
            level: WarningLevel::Warning,
            message: format!(
                "Blob {} is {:.2} MiB (threshold {:.2} MiB).",
                blob.oid,
                to_mib(blob.size),
                to_mib(thresholds.warn_blob_bytes)
            ),
            recommendation: Some(
                "Track large files with Git-LFS or store them outside the repository.".to_string(),
            ),
        });
    }
    if let Some(top) = metrics.duplicate_blobs.first() {
        if top.paths >= thresholds.warn_duplicate_paths {
            warnings.push(Warning {
        level: WarningLevel::Warning,
        message: format!(
          "Blob {} appears {} times in the working tree (threshold {}).", top.oid, top.paths, thresholds.warn_duplicate_paths
        ),
        recommendation: Some("Avoid repeating identical payloads; prefer build-time generation or configuration.".to_string()),
      });
        }
    }
    if metrics.max_commit_parents > thresholds.warn_max_parents {
        warnings.push(Warning {
            level: WarningLevel::Info,
            message: format!(
        "Commit with {} parents detected (threshold {}). Octopus merges can complicate history.",
        metrics.max_commit_parents,
        thresholds.warn_max_parents
      ),
            recommendation: Some(
                "Consider rebasing large merge trains or splitting history to simplify traversal."
                    .to_string(),
            ),
        });
    }
    for msg in &metrics.oversized_commit_messages {
        warnings.push(Warning {
            level: WarningLevel::Info,
            message: format!(
                "Commit {} has a {} byte message (threshold {}).",
                msg.oid, msg.length, thresholds.warn_commit_msg_bytes
            ),
            recommendation: Some(
                "Store large logs or dumps outside Git; keep commit messages concise.".to_string(),
            ),
        });
    }
    if warnings.is_empty() {
        warnings.push(Warning {
            level: WarningLevel::Info,
            message: "No size-related issues detected above configured thresholds.".to_string(),
            recommendation: None,
        });
    }
    warnings
}

fn print_human(report: &AnalysisReport, cfg: &AnalyzeConfig) {
    let mut foot = FootnoteRegistry::new();
    println!("{}", banner("Repository analysis"));
    if let Some(path) = &report.metrics.workdir {
        println!("{}", path);
    }
    // Unified summary table (without concern column)
    print_section("Repository summary");
    let rows = build_summary_rows(&report.metrics);
    print_table(
        &[
            ("Name", CellAlignment::Left),
            ("Value", CellAlignment::Right),
        ],
        rows,
    );

    // (Checkout (HEAD) moved near Warnings for better layout)

    if !report.metrics.largest_blobs.is_empty() {
        println!(
            "  Top {} blobs by size:",
            format_count(report.metrics.largest_blobs.len() as u64)
        );
        let rows = report
            .metrics
            .largest_blobs
            .iter()
            .enumerate()
            .map(|(idx, blob)| {
                let rf = foot.note(&blob.oid, blob.path.as_deref());
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format!("{:.2} MiB", to_mib(blob.size))),
                    blob.path
                        .as_deref()
                        .map(Cow::Borrowed)
                        .unwrap_or(Cow::Borrowed("")),
                    Cow::Owned(rf),
                ]
            })
            .collect();
        print_table(
            &[
                ("#", CellAlignment::Right),
                ("Size", CellAlignment::Right),
                ("Path", CellAlignment::Left),
                ("OID", CellAlignment::Center),
            ],
            rows,
        );
    }
    if !report.metrics.largest_trees.is_empty() {
        println!(
            "  Top {} trees by size:",
            format_count(report.metrics.largest_trees.len() as u64)
        );
        let rows = report
            .metrics
            .largest_trees
            .iter()
            .enumerate()
            .map(|(idx, tree)| {
                let rf = foot.note(&tree.oid, None);
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format!("{:.2} KiB", tree.size as f64 / 1024.0)),
                    Cow::Owned(rf),
                ]
            })
            .collect();
        print_table(
            &[
                ("#", CellAlignment::Right),
                ("Size", CellAlignment::Right),
                ("OID", CellAlignment::Center),
            ],
            rows,
        );
    }

    if !report.metrics.duplicate_blobs.is_empty() {
        let shown = report.metrics.duplicate_blobs.len().min(cfg.top);
        println!("  Duplicate blobs (top {}):", format_count(shown as u64));
        let rows = report
            .metrics
            .duplicate_blobs
            .iter()
            .enumerate()
            .map(|(idx, dup)| {
                let rf = foot.note(&dup.oid, dup.example_path.as_deref());
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format_count(dup.paths as u64)),
                    dup.example_path
                        .as_deref()
                        .map(Cow::Borrowed)
                        .unwrap_or(Cow::Borrowed("")),
                    Cow::Owned(rf),
                ]
            })
            .collect();
        print_table(
            &[
                ("#", CellAlignment::Right),
                ("Paths", CellAlignment::Right),
                ("Path", CellAlignment::Left),
                ("OID", CellAlignment::Center),
            ],
            rows,
        );
    }
    // History oddities are summarized above; keep oversized messages as a list
    if !report.metrics.oversized_commit_messages.is_empty() {
        println!("  Oversized commit messages:");
        let rows = report
            .metrics
            .oversized_commit_messages
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let rf = foot.note(&msg.oid, None);
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format_count(msg.length as u64)),
                    Cow::Owned(rf),
                ]
            })
            .collect();
        print_table(
            &[
                ("#", CellAlignment::Right),
                ("Bytes", CellAlignment::Right),
                ("OID", CellAlignment::Center),
            ],
            rows,
        );
    }

    // Show checkout (HEAD) details just before Warnings
    let mut snapshot_rows: Vec<Vec<Cow<'_, str>>> = Vec::new();
    if let Some(dir) = &report.metrics.directory_hotspots {
        snapshot_rows.push(vec![
            Cow::Borrowed("Busiest directory"),
            Cow::Borrowed(dir.path.as_str()),
            Cow::Owned(format!("{} entries", format_count(dir.entries as u64))),
        ]);
    }
    if let Some(path) = &report.metrics.longest_path {
        snapshot_rows.push(vec![
            Cow::Borrowed("Max path length"),
            Cow::Borrowed(path.path.as_str()),
            Cow::Owned(format!("{} chars", format_count(path.length as u64))),
        ]);
    }
    if !snapshot_rows.is_empty() {
        print_section("Checkout (HEAD)");
        print_table(
            &[
                ("Metric", CellAlignment::Left),
                ("Value", CellAlignment::Left),
                ("Details", CellAlignment::Left),
            ],
            snapshot_rows,
        );
    }

    print_section("Warnings");
    let warning_rows = report
        .warnings
        .iter()
        .map(|warning| {
            // Replace 40-char OIDs in certain messages with footnote markers.
            let (msg, _maybe_ref) = humanize_warning_message(&warning.message, report, &mut foot);
            vec![
                Cow::Owned(format!("{:?}", warning.level)),
                Cow::Owned(msg),
                warning
                    .recommendation
                    .as_deref()
                    .map(Cow::Borrowed)
                    .unwrap_or(Cow::Borrowed("")),
            ]
        })
        .collect();
    print_table(
        &[
            ("Level", CellAlignment::Center),
            ("Message", CellAlignment::Left),
            ("Recommendation", CellAlignment::Left),
        ],
        warning_rows,
    );

    // Print footnotes at the end
    if !foot.is_empty() {
        print_section("Footnotes");
        for (idx, oid, context) in foot.entries {
            match context {
                Some(ctx) => println!("  [{}] {} ({})", idx, oid, ctx),
                None => println!("  [{}] {}", idx, oid),
            }
        }
    }
}

// Attempt to replace OID in a known-warning message pattern with a footnote marker.
fn humanize_warning_message(
    message: &str,
    report: &AnalysisReport,
    foot: &mut FootnoteRegistry,
) -> (String, Option<String>) {
    // Patterns handled:
    // - "Blob <40-hex> is ..."
    // - "Blob <40-hex> appears ..."
    // - "Commit <40-hex> has ..."
    let mut parts = message.split_whitespace();
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    if first == "Blob" && is_hex_40(second) {
        let ctx = find_blob_context(&report.metrics, second);
        let rf = foot.note(second, ctx.as_deref());
        let rest = message[5 + 40..].to_string(); // len("Blob ") + 40
        return (format!("Blob {}{}", rf, rest), Some(rf));
    }
    if first == "Commit" && is_hex_40(second) {
        let rf = foot.note(second, None);
        let rest = message[7 + 40..].to_string(); // len("Commit ") + 40
        return (format!("Commit {}{}", rf, rest), Some(rf));
    }
    (message.to_string(), None)
}

fn is_hex_40(s: &str) -> bool {
    if s.len() != 40 {
        return false;
    }
    s.chars().all(|c| {
        matches!(c,
            '0'..='9' | 'a'..='f' | 'A'..='F'
        )
    })
}

fn find_blob_context<'a>(metrics: &'a RepositoryMetrics, oid: &str) -> Option<String> {
    // Prefer example path if present
    if let Some(p) = metrics
        .blobs_over_threshold
        .iter()
        .find(|b| b.oid == oid)
        .and_then(|b| b.path.as_ref())
    {
        return Some(p.clone());
    }
    if let Some(p) = metrics
        .largest_blobs
        .iter()
        .find(|b| b.oid == oid)
        .and_then(|b| b.path.as_ref())
    {
        return Some(p.clone());
    }
    if let Some(p) = metrics
        .duplicate_blobs
        .iter()
        .find(|d| d.oid == oid)
        .and_then(|d| d.example_path.as_ref())
    {
        return Some(p.clone());
    }
    None
}

fn run_git_capture(repo: &Path, args: &[&str]) -> io::Result<String> {
    let out = Command::new("git")
        .current_dir(repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("git {:?} failed", args),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn read_until<R: BufRead>(reader: &mut R, byte: u8, buf: &mut Vec<u8>) -> io::Result<bool> {
    buf.clear();
    let n = reader.read_until(byte, buf)?;
    Ok(n != 0)
}

fn parent_directory(path: &str) -> Option<String> {
    let pb = Path::new(path);
    pb.parent().map(|p| {
        if p.as_os_str().is_empty() {
            String::from(".")
        } else {
            p.to_string_lossy().to_string()
        }
    })
}

fn to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn to_io_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err)
}

fn heap_to_vec(heap: BinaryHeap<Reverse<(u64, String)>>) -> Vec<ObjectStat> {
    heap.into_sorted_vec()
        .into_iter()
        .map(|Reverse((size, oid))| ObjectStat {
            oid,
            size,
            path: None,
        })
        .collect()
}

fn push_top(heap: &mut BinaryHeap<Reverse<(u64, String)>>, limit: usize, size: u64, oid: &str) {
    if limit == 0 {
        return;
    }
    let entry = Reverse((size, oid.to_string()));
    if heap.len() < limit {
        heap.push(entry);
    } else if let Some(Reverse((min_size, _))) = heap.peek() {
        if size > *min_size {
            heap.pop();
            heap.push(entry);
        }
    }
}

fn banner(title: &str) -> String {
    format!("{:=^64}", format!(" {} ", title))
}

fn print_section(title: &str) {
    println!("");
    println!("{:-^64}", format!(" {} ", title));
}

fn print_table<'a>(headers: &[(&str, CellAlignment)], rows: Vec<Vec<Cow<'a, str>>>) {
    if rows.is_empty() {
        return;
    }
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.apply_modifier(UTF8_ROUND_CORNERS);
    table.set_content_arrangement(ContentArrangement::Dynamic);

    let header_cells = headers
        .iter()
        .map(|(title, align)| {
            Cell::new(*title)
                .add_attribute(Attribute::Bold)
                .set_alignment(*align)
        })
        .collect::<Vec<_>>();
    table.set_header(header_cells);

    for row in rows {
        let cells = headers
            .iter()
            .zip(row.into_iter())
            .map(|((_, align), value)| Cell::new(value.as_ref()).set_alignment(*align))
            .collect::<Vec<_>>();
        table.add_row(cells);
    }

    for line in table.to_string().lines() {
        println!("  {}", line);
    }
}

fn format_count<T: Into<u64>>(value: T) -> String {
    let digits: Vec<char> = value.into().to_string().chars().rev().collect();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.into_iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_size_gib(bytes: u64) -> String {
    format!("{:.2} GiB", to_gib(bytes))
}

fn build_summary_rows<'a>(metrics: &'a RepositoryMetrics) -> Vec<Vec<Cow<'a, str>>> {
    let mut rows: Vec<Vec<Cow<'_, str>>> = Vec::new();

    // Overall repository size
    rows.push(vec![
        Cow::Borrowed("Overall repository size"),
        Cow::Borrowed(""),
    ]);
    // * Total objects
    rows.push(vec![
        Cow::Borrowed("  * Total objects"),
        Cow::Owned(format_count(metrics.total_objects)),
    ]);
    // * Total size
    rows.push(vec![
        Cow::Borrowed("  * Total size"),
        Cow::Owned(format_size_gib(metrics.total_size_bytes)),
    ]);
    // * Loose objects
    rows.push(vec![
        Cow::Borrowed("  * Loose objects"),
        Cow::Owned(format!(
            "{} ({} MiB)",
            format_count(metrics.loose_objects),
            format!("{:.2}", to_mib(metrics.loose_size_bytes))
        )),
    ]);
    // * Packed objects
    rows.push(vec![
        Cow::Borrowed("  * Packed objects"),
        Cow::Owned(format!(
            "{} ({} MiB)",
            format_count(metrics.packed_objects),
            format!("{:.2}", to_mib(metrics.packed_size_bytes))
        )),
    ]);

    // Objects
    rows.push(vec![Cow::Borrowed("Objects"), Cow::Borrowed("")]);
    if let Some(count) = metrics.object_types.get("commit") {
        rows.push(vec![
            Cow::Borrowed("  * Commits (count)"),
            Cow::Owned(format_count(*count)),
        ]);
    }
    if let Some(count) = metrics.object_types.get("blob") {
        rows.push(vec![
            Cow::Borrowed("  * Blobs (count)"),
            Cow::Owned(format_count(*count)),
        ]);
    }

    // References
    rows.push(vec![Cow::Borrowed("References"), Cow::Borrowed("")]);
    rows.push(vec![
        Cow::Borrowed("  * Total"),
        Cow::Owned(format_count(metrics.refs_total as u64)),
    ]);
    rows.push(vec![
        Cow::Borrowed("  * Heads"),
        Cow::Owned(format_count(metrics.refs_heads as u64)),
    ]);
    rows.push(vec![
        Cow::Borrowed("  * Tags"),
        Cow::Owned(format_count(metrics.refs_tags as u64)),
    ]);
    rows.push(vec![
        Cow::Borrowed("  * Remotes"),
        Cow::Owned(format_count(metrics.refs_remotes as u64)),
    ]);
    rows.push(vec![
        Cow::Borrowed("  * Other"),
        Cow::Owned(format_count(metrics.refs_other as u64)),
    ]);

    // History structure
    rows.push(vec![Cow::Borrowed("History"), Cow::Borrowed("")]);
    rows.push(vec![
        Cow::Borrowed("  * Max parents"),
        Cow::Owned(format_count(metrics.max_commit_parents as u64)),
    ]);

    // Trees
    rows.push(vec![Cow::Borrowed("Trees"), Cow::Borrowed("")]);
    if let Some(count) = metrics.object_types.get("tree") {
        rows.push(vec![
            Cow::Borrowed("  * Trees (count)"),
            Cow::Owned(format_count(*count)),
        ]);
    }
    rows.push(vec![
        Cow::Borrowed("  * Trees total size"),
        Cow::Owned(format!("{:.2} GiB", to_gib(metrics.tree_total_size_bytes))),
    ]);

    rows
}
