use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, CellAlignment,
    ContentArrangement, Table,
};
use serde::Serialize;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::gitutil;
use crate::opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};

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
    let mut metrics = RepositoryMetrics {
        workdir: Some(repo.display().to_string()),
        ..Default::default()
    };

    eprintln!("[*] Starting repository analysis...");

    // First, get all blob sizes in one pass
    eprintln!("[*] Gathering blob sizes...");
    let (unpacked_size, packed_size) = gather_all_blob_sizes(repo)?;

    // Initialize metrics with blob sizes - pre-allocate reasonable capacities
    let estimated_blobs = unpacked_size.len();
    let _blob_paths: HashMap<String, Vec<String>> = HashMap::new();
    let mut stats = StatsCollection {
        blob_paths: HashMap::with_capacity(estimated_blobs),
        all_names: HashSet::with_capacity(estimated_blobs * 2), // Rough estimate
        num_commits: 0,
        max_parents: 0,
    };

    // Then process commit history
    eprintln!("[*] Processing commit history...");
    gather_commit_history(repo, &mut stats)?;

    // Determine maximum number of parents across all commits
    if let Ok(maxp) = gather_max_parents(repo) {
        stats.max_parents = maxp;
    }

    // Now map blob OIDs to paths efficiently using the collected blob sizes
    eprintln!("[*] Mapping blob paths (streaming)...");
    let blob_oids: HashSet<String> = unpacked_size.keys().cloned().collect();

    // Use streaming approach to avoid loading all objects into memory
    let mut blob_path_map: HashMap<String, String> = HashMap::new();
    let (mut reader, mut child) =
        run_git_capture_stream(repo, &["rev-list", "--objects", "--all"])?;

    let mut line_buf = String::new();
    while reader.read_line(&mut line_buf)? > 0 {
        let line = line_buf.trim_end();
        let mut parts = line.splitn(2, ' ');
        if let (Some(oid), Some(path)) = (parts.next(), parts.next()) {
            if blob_oids.contains(oid) && !path.is_empty() {
                blob_path_map.insert(oid.to_string(), path.to_string());
                if blob_path_map.len() >= blob_oids.len() {
                    break;
                }
            }
        }
        line_buf.clear();
    }

    // Wait for git command to complete
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("git rev-list --objects --all failed: {}", status),
        ));
    }

    eprintln!("[*] Found {} blob-to-path mappings", blob_path_map.len());

    // Convert path map (oid -> path) to blob_paths structure (oid -> Vec<path>)
    for (oid, path) in blob_path_map {
        stats
            .blob_paths
            .entry(oid.clone())
            .or_default()
            .push(path.clone());
        stats.all_names.insert(path);
    }

    // Quick repository stats
    gather_footprint(repo, &mut metrics)?;
    gather_refs(repo, &mut metrics)?;

    // Update metrics from gathered data
    metrics
        .object_types
        .insert("blob".to_string(), stats.blob_paths.len() as u64);
    metrics
        .object_types
        .insert("commit".to_string(), stats.num_commits);
    metrics.max_commit_parents = stats.max_parents;

    // Find largest blobs and prepare path mappings
    let mut largest_blobs: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
    let mut threshold_hits: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();

    for oid in stats.blob_paths.keys() {
        let actual_size = unpacked_size
            .get(oid)
            .copied()
            .unwrap_or_else(|| packed_size.get(oid).copied().unwrap_or(0));
        push_top(&mut largest_blobs, cfg.top, actual_size, oid);
        if actual_size >= cfg.thresholds.warn_blob_bytes {
            push_top(&mut threshold_hits, cfg.top, actual_size, oid);
        }
    }

    // Convert to ObjectStat with paths
    metrics.largest_blobs = heap_to_object_stats_with_paths(largest_blobs, &stats.blob_paths);
    metrics.blobs_over_threshold =
        heap_to_object_stats_with_paths(threshold_hits, &stats.blob_paths);

    // Tree inventory via cat-file for counts and top sizes (lightweight)
    eprintln!("[*] Gathering tree inventory...");
    gather_tree_inventory(repo, cfg, &mut metrics)?;

    // Keep a quick HEAD snapshot for context (simplified)
    eprintln!("[*] Analyzing working directory...");
    gather_worktree_snapshot_simplified(repo, cfg, &mut metrics)?;

    // Gather oversized commit messages based on configured threshold
    metrics.oversized_commit_messages =
        gather_oversized_commit_messages(repo, cfg.thresholds.warn_commit_msg_bytes)?;

    eprintln!("[*] Analysis complete!");
    Ok(metrics)
}

struct StatsCollection {
    blob_paths: HashMap<String, Vec<String>>,
    all_names: HashSet<String>,
    num_commits: u64,
    max_parents: usize,
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

fn gather_worktree_snapshot_simplified(
    repo: &Path,
    _cfg: &AnalyzeConfig,
    metrics: &mut RepositoryMetrics,
) -> io::Result<()> {
    let head = run_git_capture(repo, &["rev-parse", "--verify", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if head.is_empty() {
        return Ok(());
    }

    // Use git ls-tree with a simpler format to get basic stats
    let output = run_git_capture(repo, &["ls-tree", "-r", "--full-tree", &head])?;
    let mut directories: HashMap<String, usize> = HashMap::new();
    let mut longest_path_len = 0;
    let mut longest_path = String::new();

    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let _mode = parts.next().unwrap_or("");
        let typ = parts.next().unwrap_or("");
        let _oid = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("");

        if typ == "blob" {
            let len = path.len();
            if len > longest_path_len {
                longest_path_len = len;
                longest_path = path.to_string();
            }
        }

        if let Some(dir) = parent_directory(path) {
            *directories.entry(dir).or_insert(0) += 1;
        } else {
            *directories.entry(String::from(".")).or_insert(0) += 1;
        }
    }

    // Update metrics with collected data
    if !longest_path.is_empty() {
        metrics.longest_path = Some(PathStat {
            path: longest_path,
            length: longest_path_len,
        });
    }

    if let Some((path, entries)) = directories.into_iter().max_by_key(|(_, count)| *count) {
        metrics.directory_hotspots = Some(DirectoryStat { path, entries });
    }

    Ok(())
}

// History-wide metrics via single rev-list | diff-tree pipeline
fn gather_all_blob_sizes(repo: &Path) -> io::Result<(HashMap<String, u64>, HashMap<String, u64>)> {
    let output = run_git_capture(
        repo,
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize) %(objectsize:disk)",
            "--batch-all-objects",
        ],
    )?;

    // Count total lines first for progress bar
    let lines: Vec<&str> = output.lines().collect();
    let total_lines = lines.len();

    // Pre-allocate with reasonable capacity based on typical repository size
    let mut unpacked_size = HashMap::with_capacity(100_000);
    let mut packed_size = HashMap::with_capacity(100_000);
    let mut blob_count = 0;
    let mut processed_objects = 0;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Use more efficient parsing - avoid Vec allocation
        let mut parts_iter = trimmed.split_whitespace();
        if let (Some(sha), Some(objtype), Some(objsize_str), Some(objdisksize_str)) = (
            parts_iter.next(),
            parts_iter.next(),
            parts_iter.next(),
            parts_iter.next(),
        ) {
            if objtype == "blob" {
                if let (Ok(objsize), Ok(objdisksize)) =
                    (objsize_str.parse::<u64>(), objdisksize_str.parse::<u64>())
                {
                    unpacked_size.insert(sha.to_string(), objsize);
                    packed_size.insert(sha.to_string(), objdisksize);
                    blob_count += 1;
                }
            }
        }

        processed_objects += 1;

        // Update progress bar every 1000 items
        if idx % 1000 == 0 && total_lines > 0 {
            let progress = ((processed_objects as f64 / total_lines as f64) * 100.0) as u32;
            let bar_length = 30;
            let filled = progress as usize * bar_length / 100;
            let bar: String = (0..bar_length)
                .map(|i| if i < filled { '=' } else { ' ' })
                .collect();
            print!(
                "\r[*] Processing objects [{}] {}% ({}/{})",
                bar, progress, processed_objects, total_lines
            );
            use std::io::Write;
            std::io::stdout().flush().unwrap();
        }
    }

    // Show final progress at 100%
    if total_lines > 0 {
        let bar_length = 30;
        let bar: String = (0..bar_length).map(|_| '=').collect();
        println!(
            "\r[*] Processing objects [{}] 100% ({}/{})",
            bar, processed_objects, total_lines
        );
        use std::io::Write;
        std::io::stdout().flush().unwrap();
    }

    eprintln!(
        "[*] Found {} blobs out of {} total objects",
        blob_count, processed_objects
    );
    Ok((unpacked_size, packed_size))
}

fn gather_commit_history(repo: &Path, stats: &mut StatsCollection) -> io::Result<()> {
    // Use a more efficient batch processing approach
    // Process commits in batches of 5000 to balance memory and performance
    const BATCH_SIZE: usize = 5000;

    // Get total commit count first
    let rev_list_output = run_git_capture(repo, &["rev-list", "--all", "--count"])?;
    let total_commits = rev_list_output
        .trim()
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Could not parse commit count"))?;

    let mut processed = 0;
    while processed < total_commits {
        let batch_start = processed;
        let batch_end = std::cmp::min(processed + BATCH_SIZE, total_commits);

        // Get a batch of commits using skip and max-count
        let batch_output = run_git_capture(
            repo,
            &[
                "rev-list",
                "--all",
                "--skip",
                &batch_start.to_string(),
                "--max-count",
                &(batch_end - batch_start).to_string(),
            ],
        )?;

        let commits: Vec<&str> = batch_output.split_whitespace().collect();

        // Process this batch using a single git log command
        if !commits.is_empty() {
            let commit_range = format!("{}..{}", commits[0], commits[commits.len() - 1]);
            let log_output = run_git_capture(
                repo,
                &[
                    "log",
                    "--pretty=format:%H %P",
                    "--name-status",
                    "--no-renames",
                    &commit_range,
                ],
            )?;

            // Parse the batch output
            parse_batch_log_output(&log_output, stats, repo)?;
            processed += commits.len();

            // Update progress bar after each batch
            let progress = ((processed as f64 / total_commits as f64) * 100.0) as u32;
            let bar_length = 30;
            let filled = progress as usize * bar_length / 100;
            let bar: String = (0..bar_length)
                .map(|i| if i < filled { '=' } else { ' ' })
                .collect();
            print!(
                "\r[*] Processing commits [{}] {}% ({}/{})",
                bar, progress, processed, total_commits
            );
            use std::io::Write;
            std::io::stdout().flush().unwrap();
        } else {
            break;
        }
    }

    // Clear progress line and show final result
    println!();
    stats.num_commits = total_commits as u64;
    eprintln!(
        "[*] Commit processing completed. Total: {}",
        stats.num_commits
    );
    Ok(())
}

fn gather_max_parents(repo: &Path) -> io::Result<usize> {
    let (mut reader, mut child) =
        run_git_capture_stream(repo, &["rev-list", "--parents", "--all"])?;
    let mut max_parents: usize = 0;
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        let count = line.split_whitespace().count();
        if count > 0 {
            let parents = count - 1; // first is commit itself
            if parents > max_parents {
                max_parents = parents;
            }
        }
        line.clear();
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("git rev-list --parents --all failed: {}", status),
        ));
    }

    Ok(max_parents)
}

fn gather_oversized_commit_messages(
    repo: &Path,
    threshold_bytes: usize,
) -> io::Result<Vec<CommitMessageStat>> {
    if threshold_bytes == 0 {
        return Ok(Vec::new());
    }
    let output = run_git_capture(repo, &["log", "--all", "--pretty=%H%x00%B%x00"])?;
    let mut stats = Vec::new();
    let mut iter = output.split('\0');
    while let Some(oid) = iter.next() {
        if oid.is_empty() {
            break;
        }
        if let Some(msg) = iter.next() {
            let len = msg.len();
            if len >= threshold_bytes {
                stats.push(CommitMessageStat {
                    oid: oid.trim().to_string(),
                    length: len,
                });
            }
        } else {
            break;
        }
    }
    Ok(stats)
}

fn parse_batch_log_output(
    output: &str,
    stats: &mut StatsCollection,
    _repo: &Path,
) -> io::Result<()> {
    let lines: Vec<&str> = output.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Skip empty lines
        if line.is_empty() {
            i += 1;
            continue;
        }

        // Check if this is a commit line. We allow either a bare 40-hex hash
        // or a hash followed by one or more parent hashes ("%H %P" output).
        let is_commit_line = if line.len() >= 40 {
            let (head, rest) = line.split_at(40);
            head.chars().all(|c| c.is_ascii_hexdigit())
                && (rest.is_empty() || rest.starts_with(' '))
        } else {
            false
        };

        if is_commit_line {
            // Parse commit and optional parents from the line
            let mut parts = line.split_whitespace();
            let commit = parts.next().unwrap_or("").to_string();
            let parents: Vec<String> = parts.map(|s| s.to_string()).collect();
            i += 1;

            // Collect file changes for this commit
            let mut file_changes = Vec::new();

            while i < lines.len() {
                let change_line = lines[i].trim();

                // Stop if we hit the next commit or empty line
                if change_line.is_empty()
                    || (change_line.len() == 40
                        && change_line.chars().all(|c| c.is_ascii_hexdigit()))
                {
                    break;
                }

                // Parse file change: <status> <path>
                let mut parts = change_line.split_whitespace();
                if let (Some(status), Some(path)) = (parts.next(), parts.next()) {
                    // Don't add placeholder hashes to stats - blob mapping will be done later
                    // Just track file changes for commit processing
                    let placeholder_id = format!("placeholder_{}", path.len());

                    file_changes.push((
                        vec!["100644".to_string()], // default mode
                        vec![placeholder_id], // placeholder that won't interfere with real hashes
                        status.to_string(),
                        vec![path.to_string()],
                    ));
                }

                i += 1;
            }

            // Analyze this commit
            analyze_commit(stats, commit, parents, file_changes);
            stats.num_commits += 1;
        } else {
            i += 1;
        }
    }

    Ok(())
}

// Type alias to reduce complexity
type FileChange = (Vec<String>, Vec<String>, String, Vec<String>);

fn analyze_commit(
    stats: &mut StatsCollection,
    _commit: String,
    parents: Vec<String>,
    file_changes: Vec<FileChange>,
) {
    // Track max parents seen
    if stats.max_parents < parents.len() {
        stats.max_parents = parents.len();
    }
    for change in file_changes {
        let (modes, shas, change_types, filenames) = change;
        let mode = &modes[modes.len() - 1];
        let sha = &shas[shas.len() - 1];
        let filename = &filenames[filenames.len() - 1];

        // Skip submodules and deletions
        if mode == "160000" || mode == "000000" {
            continue;
        }

        // Track deletions - more efficient check
        let has_additions = !change_types.is_empty()
            && change_types
                .bytes()
                .any(|c| matches!(c, b'A' | b'M' | b'T'));
        if !has_additions {
            continue;
        }

        // Record blob paths - use the hash as-is to avoid to_ascii_lowercase allocation
        // Git hashes are already lowercase in most cases, and case insensitivity isn't critical for analysis
        let paths_entry = stats.blob_paths.entry(sha.clone()).or_default();
        paths_entry.push(filename.clone());
        stats.all_names.insert(filename.clone());
    }
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

fn print_human(report: &AnalysisReport, _cfg: &AnalyzeConfig) {
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
    s.chars().all(|c| c.is_ascii_hexdigit())
}

fn find_blob_context(metrics: &RepositoryMetrics, oid: &str) -> Option<String> {
    // Prefer example path if present
    metrics
        .blobs_over_threshold
        .iter()
        .find(|b| b.oid == oid)
        .and_then(|b| b.path.as_ref())
        .or_else(|| {
            metrics
                .largest_blobs
                .iter()
                .find(|b| b.oid == oid)
                .and_then(|b| b.path.as_ref())
        })
        .cloned()
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

/// Stream-based git command runner for memory-efficient processing.
///
/// This function can replace run_git_capture when processing large outputs
/// to avoid loading the entire output into memory.
///
/// Returns a tuple of (BufReader, Child) so caller can wait on the child
/// to ensure the command succeeded.
fn run_git_capture_stream(
    repo: &Path,
    args: &[&str],
) -> io::Result<(BufReader<ChildStdout>, Child)> {
    let mut cmd = Command::new("git")
        .current_dir(repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = cmd
        .stdout
        .take()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture git stdout"))?;
    Ok((BufReader::new(stdout), cmd))
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

fn heap_to_object_stats_with_paths(
    heap: BinaryHeap<Reverse<(u64, String)>>,
    blob_paths: &HashMap<String, Vec<String>>,
) -> Vec<ObjectStat> {
    heap.into_sorted_vec()
        .into_iter()
        .map(|Reverse((size, oid))| {
            let path = blob_paths
                .get(&oid)
                .and_then(|paths| paths.first().cloned());
            ObjectStat { oid, size, path }
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
    println!();
    println!("{:-^64}", format!(" {} ", title));
}

fn print_table(headers: &[(&str, CellAlignment)], rows: Vec<Vec<Cow<'_, str>>>) {
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

fn build_summary_rows(metrics: &RepositoryMetrics) -> Vec<Vec<Cow<'_, str>>> {
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
            "{} ({:.2} MiB)",
            format_count(metrics.loose_objects),
            to_mib(metrics.loose_size_bytes)
        )),
    ]);
    // * Packed objects
    rows.push(vec![
        Cow::Borrowed("  * Packed objects"),
        Cow::Owned(format!(
            "{} ({:.2} MiB)",
            format_count(metrics.packed_objects),
            to_mib(metrics.packed_size_bytes)
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
