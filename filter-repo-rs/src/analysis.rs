use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, CellAlignment,
    ContentArrangement, Table,
};
use serde::Serialize;
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Instant;

use crate::gitutil;
use crate::opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};

mod term_colors {
    use std::io::IsTerminal;

    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GREEN: &str = "\x1b[32m";

    pub fn supports_color() -> bool {
        std::io::stdout().is_terminal() || std::env::var("FORCE_COLOR").is_ok()
    }

    pub fn eprintln_color(color: &str, msg: &str) {
        if supports_color() {
            eprintln!("{}{}{}", color, msg, RESET);
        } else {
            eprintln!("{}", msg);
        }
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
pub struct FileStat {
    pub path: String,
    pub size: u64,
    pub versions: usize,
    pub largest_oid: String,
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
    pub largest_files: Vec<FileStat>,
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
    let _start_time = Instant::now();
    let mut metrics = RepositoryMetrics {
        workdir: Some(repo.display().to_string()),
        ..Default::default()
    };

    term_colors::eprintln_color(term_colors::CYAN, "[*] Starting repository analysis...");

    // First, get all blob sizes in one pass
    term_colors::eprintln_color(term_colors::CYAN, "[*] Gathering blob sizes...");
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
    term_colors::eprintln_color(term_colors::CYAN, "[*] Processing commit history...");
    gather_commit_history(repo, &mut stats)?;

    // Determine maximum number of parents across all commits
    if let Ok(maxp) = gather_max_parents(repo) {
        stats.max_parents = maxp;
    }

    // Now map blob OIDs to paths efficiently using the collected blob sizes
    term_colors::eprintln_color(term_colors::CYAN, "[*] Mapping blob paths (streaming)...");
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
        return Err(io::Error::other(format!(
            "git rev-list --objects --all failed: {}",
            status
        )));
    }

    term_colors::eprintln_color(
        term_colors::GREEN,
        &format!("[*] Found {} blob-to-path mappings", blob_path_map.len()),
    );

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

    // Group blobs by file path to find unique files
    metrics.largest_files =
        compute_largest_files(&stats.blob_paths, &unpacked_size, &packed_size, cfg.top);

    // Tree inventory via cat-file for counts and top sizes (lightweight)
    term_colors::eprintln_color(term_colors::CYAN, "[*] Gathering tree inventory...");

    // Keep a quick HEAD snapshot for context (simplified)
    term_colors::eprintln_color(term_colors::CYAN, "[*] Analyzing working directory...");

    // Gather oversized commit messages based on configured threshold
    metrics.oversized_commit_messages =
        gather_oversized_commit_messages(repo, cfg.thresholds.warn_commit_msg_bytes)?;

    term_colors::eprintln_color(term_colors::GREEN, "[*] Analysis complete!");
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

#[cfg(test)]
fn collect_blob_sizes_from_reader<R: BufRead>(
    reader: &mut R,
) -> io::Result<(HashMap<String, u64>, HashMap<String, u64>, usize)> {
    let mut unpacked_size = HashMap::new();
    let mut packed_size = HashMap::new();
    let mut processed_objects = 0usize;
    let mut line_buf = String::new();

    while reader.read_line(&mut line_buf)? > 0 {
        let trimmed = line_buf.trim();
        if !trimmed.is_empty() {
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
                    }
                }
            }
            processed_objects += 1;
        }
        line_buf.clear();
    }

    Ok((unpacked_size, packed_size, processed_objects))
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

// History-wide metrics via single rev-list | diff-tree pipeline
fn gather_all_blob_sizes(repo: &Path) -> io::Result<(HashMap<String, u64>, HashMap<String, u64>)> {
    let start_time = Instant::now();
    let (mut reader, mut child) = run_git_capture_stream(
        repo,
        &[
            "cat-file",
            "--batch-check=%(objectname) %(objecttype) %(objectsize) %(objectsize:disk)",
            "--batch-all-objects",
        ],
    )?;

    // Pre-allocate with reasonable capacity based on typical repository size.
    let mut unpacked_size = HashMap::with_capacity(100_000);
    let mut packed_size = HashMap::with_capacity(100_000);
    let mut blob_count = 0usize;
    let mut processed_objects = 0usize;
    let mut progress_output_enabled = true;
    let mut line_buf = String::new();

    while reader.read_line(&mut line_buf)? > 0 {
        let trimmed = line_buf.trim();
        if !trimmed.is_empty() {
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

            // Keep lightweight progress reporting without requiring full output buffering.
            if progress_output_enabled && processed_objects.is_multiple_of(1000) {
                let elapsed = start_time.elapsed();
                let rate = if elapsed.as_secs_f64() > 0.0 {
                    processed_objects as f64 / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                progress_output_enabled = write_progress_stdout(format_args!(
                    "\r[*] Processing objects {} {} ({:.0}/s)",
                    processed_objects,
                    format_elapsed(elapsed),
                    rate
                ))?;
            }
        }
        line_buf.clear();
    }

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "git cat-file --batch-all-objects failed: {}",
            status
        )));
    }

    if processed_objects > 0 && progress_output_enabled {
        let elapsed = start_time.elapsed();
        let rate = if elapsed.as_secs_f64() > 0.0 {
            processed_objects as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };
        let _ = write_progress_stdout(format_args!(
            "\r[*] Processing objects {} {} ({:.0}/s)\n",
            processed_objects,
            format_elapsed(elapsed),
            rate
        ))?;
    }

    eprintln!(
        "[*] Found {} blobs out of {} total objects",
        blob_count, processed_objects
    );
    Ok((unpacked_size, packed_size))
}

fn gather_commit_history(repo: &Path, stats: &mut StatsCollection) -> io::Result<()> {
    // Use streaming approach: process all commits in a single git log command
    // This is more efficient than batched --skip approach which is O(n²)
    term_colors::eprintln_color(
        term_colors::CYAN,
        "[*] Gathering commit history (streaming)...",
    );

    // Get total commit count first
    let rev_list_output = run_git_capture(repo, &["rev-list", "--all", "--count"])?;
    let total_commits = rev_list_output
        .trim()
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Could not parse commit count"))?;

    // Stream commit data in a single pass
    let (mut reader, mut child) = run_git_capture_stream(
        repo,
        &[
            "log",
            "--all",
            "--pretty=format:%H %P",
            "--name-status",
            "--no-renames",
        ],
    )?;

    let mut line_buf = String::new();
    let mut processed: usize = 0;
    let mut commit_data: Vec<String> = Vec::new();
    let mut progress_output_enabled = true;

    while reader.read_line(&mut line_buf)? > 0 {
        let line = line_buf.trim_end();
        if line.is_empty() {
            // End of commit block, process accumulated data
            if !commit_data.is_empty() {
                process_commit_block(&commit_data, stats)?;
                commit_data.clear();
                processed += 1;

                // Progress indicator every 1000 commits
                if progress_output_enabled && processed.is_multiple_of(1000) {
                    let progress = ((processed as f64 / total_commits as f64) * 100.0) as u32;
                    let bar_length = 30;
                    let filled = progress as usize * bar_length / 100;
                    let bar: String = (0..bar_length)
                        .map(|i| if i < filled { '=' } else { ' ' })
                        .collect();
                    progress_output_enabled = write_progress_stdout(format_args!(
                        "\r[*] Processing commits [{}] {}% ({}/{})",
                        bar, progress, processed, total_commits
                    ))?;
                }
            }
        } else {
            commit_data.push(line.to_string());
        }
        line_buf.clear();
    }

    // Process last commit if exists
    if !commit_data.is_empty() {
        process_commit_block(&commit_data, stats)?;
    }

    // Wait for git command to complete
    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "git log --all failed: {}",
            status
        )));
    }

    // Clear progress line and show final result
    if progress_output_enabled {
        let _ = write_progress_stdout(format_args!("\n"))?;
    }
    stats.num_commits = total_commits as u64;
    eprintln!(
        "[*] Commit processing completed. Total: {}",
        stats.num_commits
    );
    Ok(())
}

/// Process a single commit block from git log output
fn process_commit_block(commit_data: &[String], stats: &mut StatsCollection) -> io::Result<()> {
    if commit_data.is_empty() {
        return Ok(());
    }

    // First line contains commit hash and parent hashes
    let first_line = &commit_data[0];
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(());
    }

    let parent_count = parts.len().saturating_sub(1);
    if stats.max_parents < parent_count {
        stats.max_parents = parent_count;
    }
    stats.num_commits += 1;

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
        return Err(io::Error::other(format!(
            "git rev-list --parents --all failed: {}",
            status
        )));
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
    let (mut reader, mut child) =
        run_git_capture_stream(repo, &["log", "--all", "--pretty=%H%x00%B%x00"])?;
    let stats = collect_oversized_commit_messages_from_reader(&mut reader, threshold_bytes)?;

    let status = child.wait()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "git log --all --pretty=%H%x00%B%x00 failed: {}",
            status
        )));
    }

    Ok(stats)
}

fn collect_oversized_commit_messages_from_reader<R: BufRead>(
    reader: &mut R,
    threshold_bytes: usize,
) -> io::Result<Vec<CommitMessageStat>> {
    if threshold_bytes == 0 {
        return Ok(Vec::new());
    }

    let mut stats = Vec::new();
    let mut oid_buf = Vec::new();
    let mut msg_buf = Vec::new();

    loop {
        oid_buf.clear();
        let oid_read = reader.read_until(0, &mut oid_buf)?;
        if oid_read == 0 {
            break;
        }
        if oid_buf.last() == Some(&0) {
            oid_buf.pop();
        }
        if oid_buf.is_empty() {
            break;
        }

        msg_buf.clear();
        let msg_read = reader.read_until(0, &mut msg_buf)?;
        if msg_read == 0 {
            break;
        }
        if msg_buf.last() == Some(&0) {
            msg_buf.pop();
        }

        if msg_buf.len() >= threshold_bytes {
            stats.push(CommitMessageStat {
                oid: String::from_utf8_lossy(&oid_buf).trim().to_string(),
                length: msg_buf.len(),
            });
        }
    }

    Ok(stats)
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

    // Show largest files (unique files, grouped by path) instead of individual blob versions
    if !report.metrics.largest_files.is_empty() {
        println!(
            "  Top {} files by size:",
            format_count(report.metrics.largest_files.len() as u64)
        );
        let rows = report
            .metrics
            .largest_files
            .iter()
            .enumerate()
            .map(|(idx, file)| {
                let truncated_oid = format!("{:.8}", file.largest_oid);
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format!("{:.2} MiB", to_mib(file.size))),
                    Cow::Owned(file.path.clone()),
                    Cow::Owned(format!("{} ver", file.versions)),
                    Cow::Owned(truncated_oid),
                ]
            })
            .collect();
        print_table(
            &[
                ("#", CellAlignment::Right),
                ("Size", CellAlignment::Right),
                ("Path", CellAlignment::Left),
                ("Vers", CellAlignment::Center),
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
                let truncated_oid = format!("{:.8}", tree.oid);
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format!("{:.2} KiB", tree.size as f64 / 1024.0)),
                    Cow::Owned(truncated_oid),
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
                let truncated_oid = format!("{:.8}", msg.oid);
                vec![
                    Cow::Owned(format!("{}", idx + 1)),
                    Cow::Owned(format_count(msg.length as u64)),
                    Cow::Owned(truncated_oid),
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
            let (msg, _maybe_ref) = humanize_warning_message(&warning.message, report);
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
}

// Attempt to replace OID in a known-warning message pattern with a footnote marker.
fn humanize_warning_message(message: &str, report: &AnalysisReport) -> (String, Option<String>) {
    // Patterns handled:
    // - "Blob <40-hex> is ..."
    // - "Blob <40-hex> appears ..."
    // - "Commit <40-hex> has ..."
    let mut parts = message.split_whitespace();
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    if first == "Blob" && is_hex_40(second) {
        let ctx = find_blob_context(&report.metrics, second);
        let truncated = format!("{:.8}", second);
        return (
            format!("Blob {} ({})", truncated, ctx.unwrap_or_default()),
            Some(truncated),
        );
    }
    if first == "Commit" && is_hex_40(second) {
        let truncated = format!("{:.8}", second);
        return (format!("Commit {}", truncated), Some(truncated));
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
        return Err(io::Error::other(format!("git {:?} failed", args)));
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
        .ok_or_else(|| io::Error::other("failed to capture git stdout"))?;
    Ok((BufReader::new(stdout), cmd))
}

fn flush_progress_writer<W: Write>(writer: &mut W) -> io::Result<bool> {
    match writer.flush() {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(err) => Err(err),
    }
}

fn write_progress_stdout(args: std::fmt::Arguments<'_>) -> io::Result<bool> {
    // Progress belongs on stderr so machine-readable stdout (e.g. --analyze-json) stays clean.
    let mut stderr = io::stderr();
    match stderr.write_fmt(args) {
        Ok(()) => flush_progress_writer(&mut stderr),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(err) => Err(err),
    }
}

fn to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn to_io_error(err: serde_json::Error) -> io::Error {
    io::Error::other(err)
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

fn compute_largest_files(
    blob_paths: &HashMap<String, Vec<String>>,
    unpacked_size: &HashMap<String, u64>,
    packed_size: &HashMap<String, u64>,
    top: usize,
) -> Vec<FileStat> {
    if top == 0 {
        return Vec::new();
    }

    let mut file_map: HashMap<String, (u64, String, usize)> = HashMap::new();

    for (oid, paths) in blob_paths {
        let size = unpacked_size
            .get(oid)
            .copied()
            .unwrap_or_else(|| packed_size.get(oid).copied().unwrap_or(0));

        for path in paths {
            let entry = file_map.entry(path.clone()).or_insert((0, oid.clone(), 0));
            if size > entry.0 {
                entry.0 = size;
                entry.1 = oid.clone();
            }
            entry.2 += 1;
        }
    }

    let mut files: Vec<FileStat> = file_map
        .into_iter()
        .map(|(path, (size, largest_oid, versions))| FileStat {
            path,
            size,
            versions,
            largest_oid,
        })
        .collect();

    files.sort_by(|a, b| b.size.cmp(&a.size));
    files.truncate(top);
    files
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
    if term_colors::supports_color() {
        format!(
            "{}{}{:=^64}{}",
            term_colors::BOLD,
            term_colors::CYAN,
            format!(" {} ", title),
            term_colors::RESET
        )
    } else {
        format!("{:=^64}", format!(" {} ", title))
    }
}

fn print_section(title: &str) {
    println!();
    if term_colors::supports_color() {
        println!(
            "{}{}{:-^64}{}",
            term_colors::BOLD,
            term_colors::CYAN,
            format!(" {} ", title),
            term_colors::RESET
        )
    } else {
        println!("{:-^64}", format!(" {} ", title));
    }
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

fn format_elapsed(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{:.1}s", secs as f64 + duration.subsec_nanos() as f64 / 1e9)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
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

#[cfg(test)]
mod tests {
    use super::{
        collect_blob_sizes_from_reader, collect_oversized_commit_messages_from_reader,
        flush_progress_writer,
    };
    use std::io::{Cursor, ErrorKind, Write};

    struct ErrorWriter {
        kind: ErrorKind,
    }

    impl Write for ErrorWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(self.kind, "forced flush error"))
        }
    }

    #[test]
    fn collect_blob_sizes_from_reader_tracks_only_blob_entries() {
        let input = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa blob 10 8
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb tree 7 5
cccccccccccccccccccccccccccccccccccccccc blob 42 21
";
        let mut reader = Cursor::new(input.as_bytes());

        let (unpacked, packed, processed) =
            collect_blob_sizes_from_reader(&mut reader).expect("parse batch output");

        assert_eq!(processed, 3, "expected all non-empty lines to be processed");
        assert_eq!(unpacked.len(), 2, "expected only blob entries");
        assert_eq!(packed.len(), 2, "expected only blob entries");
        assert_eq!(
            unpacked.get("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(&10)
        );
        assert_eq!(
            packed.get("cccccccccccccccccccccccccccccccccccccccc"),
            Some(&21)
        );
    }

    #[test]
    fn collect_blob_sizes_from_reader_skips_malformed_or_invalid_sizes() {
        let input = "\
invalid line
dddddddddddddddddddddddddddddddddddddddd blob NaN 1
eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee blob 11 not-a-number
ffffffffffffffffffffffffffffffffffffffff blob 12 6
";
        let mut reader = Cursor::new(input.as_bytes());

        let (unpacked, packed, processed) =
            collect_blob_sizes_from_reader(&mut reader).expect("parse batch output");

        assert_eq!(processed, 4);
        assert_eq!(unpacked.len(), 1);
        assert_eq!(packed.len(), 1);
        assert_eq!(
            unpacked.get("ffffffffffffffffffffffffffffffffffffffff"),
            Some(&12)
        );
        assert_eq!(
            packed.get("ffffffffffffffffffffffffffffffffffffffff"),
            Some(&6)
        );
    }

    #[test]
    fn flush_progress_writer_treats_broken_pipe_as_non_fatal() {
        let mut writer = ErrorWriter {
            kind: ErrorKind::BrokenPipe,
        };
        let result = flush_progress_writer(&mut writer);
        assert!(result.is_ok(), "BrokenPipe should not propagate as error");
        assert!(
            !result.expect("BrokenPipe should map to non-fatal false"),
            "BrokenPipe should return false to indicate no further progress output"
        );
    }

    #[test]
    fn flush_progress_writer_propagates_other_flush_errors() {
        let mut writer = ErrorWriter {
            kind: ErrorKind::PermissionDenied,
        };
        let result = flush_progress_writer(&mut writer);
        assert!(
            result.is_err(),
            "non-BrokenPipe flush errors should propagate"
        );
    }

    #[test]
    fn collect_oversized_commit_messages_from_reader_filters_by_threshold() {
        let input = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0short\0\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\0this message is long enough\0";
        let mut reader = Cursor::new(&input[..]);

        let stats =
            collect_oversized_commit_messages_from_reader(&mut reader, 10).expect("parse stream");

        assert_eq!(stats.len(), 1, "expected only one message above threshold");
        assert_eq!(
            stats[0].oid, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "expected longer message oid to be captured"
        );
        assert!(
            stats[0].length >= 10,
            "expected captured message length >= threshold"
        );
    }

    #[test]
    fn collect_oversized_commit_messages_from_reader_ignores_truncated_pairs() {
        let input = b"cccccccccccccccccccccccccccccccccccccccc\0";
        let mut reader = Cursor::new(&input[..]);

        let stats =
            collect_oversized_commit_messages_from_reader(&mut reader, 1).expect("parse stream");

        assert!(
            stats.is_empty(),
            "truncated oid/message pair should be ignored without panic"
        );
    }
}
