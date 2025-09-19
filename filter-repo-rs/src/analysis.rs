use comfy_table::{
  modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Attribute, Cell, CellAlignment,
  ContentArrangement, Table,
};
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::opts::{AnalyzeConfig, AnalyzeThresholds, Mode, Options};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WarningLevel { Info, Warning, Critical }

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
  gather_object_inventory(repo, cfg, &mut metrics)?;
  gather_refs(repo, &mut metrics)?;
  gather_worktree_snapshot(repo, cfg, &mut metrics)?;
  gather_history_stats(repo, cfg, &mut metrics)?;
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

fn gather_object_inventory(repo: &Path, cfg: &AnalyzeConfig, metrics: &mut RepositoryMetrics) -> io::Result<()> {
  let mut largest_blobs: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
  let mut largest_trees: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
  let mut threshold_hits: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
  let mut object_counts: BTreeMap<String, u64> = BTreeMap::new();
  let mut child = Command::new("git")
    .current_dir(repo)
    .arg("cat-file")
    .arg("--batch-check")
    .arg("--batch-all-objects")
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()?;
  let stdout = child.stdout.take().ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture git cat-file stdout"))?;
  let reader = BufReader::new(stdout);
  for line in reader.lines() {
    let line = line?;
    let mut parts = line.split_whitespace();
    let oid = parts.next().unwrap_or("");
    let typ = parts.next().unwrap_or("");
    let size = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    *object_counts.entry(typ.to_string()).or_insert(0) += 1;
    if typ == "blob" {
      push_top(&mut largest_blobs, cfg.top, size, oid);
      if size >= cfg.thresholds.warn_blob_bytes {
        push_top(&mut threshold_hits, cfg.top, size, oid);
      }
    } else if typ == "tree" {
      push_top(&mut largest_trees, cfg.top, size, oid);
    }
  }
  let status = child.wait()?;
  if !status.success() {
    return Err(io::Error::new(io::ErrorKind::Other, "git cat-file --batch-check failed"));
  }
  metrics.total_objects = object_counts.values().copied().sum();
  metrics.object_types = object_counts;
  metrics.largest_blobs = heap_to_vec(largest_blobs);
  metrics.largest_trees = heap_to_vec(largest_trees);
  metrics.blobs_over_threshold = heap_to_vec(threshold_hits);
  Ok(())
}

fn gather_refs(repo: &Path, metrics: &mut RepositoryMetrics) -> io::Result<()> {
  let output = run_git_capture(repo, &["for-each-ref", "--format=%(refname)"])?;
  for line in output.lines() {
    let name = line.trim();
    if name.is_empty() {
      continue;
    }
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

fn gather_worktree_snapshot(repo: &Path, cfg: &AnalyzeConfig, metrics: &mut RepositoryMetrics) -> io::Result<()> {
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
  let stdout = child.stdout.take().ok_or_else(|| io::Error::new(io::ErrorKind::Other, "failed to capture git ls-tree stdout"))?;
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
          *current = PathStat { path: path.to_string(), length: len };
        }
      } else {
        metrics.longest_path = Some(PathStat { path: path.to_string(), length: len });
      }
      sample_paths.entry(oid.to_string()).or_insert_with(|| path.to_string());
      let entry = duplicates.entry(oid.to_string()).or_insert_with(|| DuplicateBlobStat {
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
    }
  }
  for blob in &mut metrics.blobs_over_threshold {
    if let Some(path) = sample_paths.get(&blob.oid) {
      blob.path = Some(path.clone());
    }
  }
  if let Some((path, entries)) = directories.into_iter().max_by_key(|(_, count)| *count) {
    metrics.directory_hotspots = Some(DirectoryStat { path, entries });
  }
  Ok(())
}

fn gather_history_stats(repo: &Path, cfg: &AnalyzeConfig, metrics: &mut RepositoryMetrics) -> io::Result<()> {
  let mut child = Command::new("git")
    .current_dir(repo)
    .arg("rev-list")
    .arg("--all")
    .arg("--parents")
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()?;
  if let Some(stdout) = child.stdout.take() {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
      let line = line?;
      let parents = line.split_whitespace().skip(1).count();
      if parents > metrics.max_commit_parents {
        metrics.max_commit_parents = parents;
      }
    }
  }
  let status = child.wait()?;
  if !status.success() {
    return Err(io::Error::new(io::ErrorKind::Other, "git rev-list --parents failed"));
  }
  let mut child = Command::new("git")
    .current_dir(repo)
    .arg("log")
    .arg("--all")
    .arg("--pretty=%H%x00%B%x00")
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .spawn()?;
  if let Some(stdout) = child.stdout.take() {
    let mut reader = BufReader::new(stdout);
    let mut oid_buf = Vec::new();
    let mut msg_buf = Vec::new();
    loop {
      if !read_until(&mut reader, 0, &mut oid_buf)? {
        break;
      }
      if oid_buf.is_empty() {
        continue;
      }
      let oid = String::from_utf8_lossy(&oid_buf[..oid_buf.len().saturating_sub(1)]).to_string();
      if oid.is_empty() {
        continue;
      }
      if !read_until(&mut reader, 0, &mut msg_buf)? {
        break;
      }
      let length = msg_buf.len().saturating_sub(1);
      if length > cfg.thresholds.warn_commit_msg_bytes {
        metrics.oversized_commit_messages.push(CommitMessageStat { oid, length });
      }
    }
  }
  let status = child.wait()?;
  if !status.success() {
    return Err(io::Error::new(io::ErrorKind::Other, "git log --pretty failed"));
  }
  Ok(())
}

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
        "Repository is {:.2} GiB (warning threshold {:.2} GiB).", to_gib(metrics.total_size_bytes), to_gib(thresholds.warn_total_bytes)
      ),
      recommendation: Some("Prune large assets or split the project to keep Git operations fast.".to_string()),
    });
  }
  if metrics.refs_total >= thresholds.warn_ref_count {
    warnings.push(Warning {
      level: WarningLevel::Warning,
      message: format!(
        "Repository has {} refs (warning threshold {}).", metrics.refs_total, thresholds.warn_ref_count
      ),
      recommendation: Some("Delete stale branches/tags or move rarely-needed refs to a separate remote.".to_string()),
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
        "Blob {} is {:.2} MiB (threshold {:.2} MiB).", blob.oid, to_mib(blob.size), to_mib(thresholds.warn_blob_bytes)
      ),
      recommendation: Some("Track large files with Git-LFS or store them outside the repository.".to_string()),
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
      recommendation: Some("Consider rebasing large merge trains or splitting history to simplify traversal.".to_string()),
    });
  }
  for msg in &metrics.oversized_commit_messages {
    warnings.push(Warning {
      level: WarningLevel::Info,
      message: format!(
        "Commit {} has a {} byte message (threshold {}).",
        msg.oid,
        msg.length,
        thresholds.warn_commit_msg_bytes
      ),
      recommendation: Some("Store large logs or dumps outside Git; keep commit messages concise.".to_string()),
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
  println!("{}", banner("Repository analysis"));
  if let Some(path) = &report.metrics.workdir {
    println!("{}", path);
  }

  print_section("Footprint");
  print_table(
    &[("Metric", CellAlignment::Left), ("Count", CellAlignment::Right), ("Approx. size", CellAlignment::Right)],
    vec![
      vec![
        "Loose objects".to_string(),
        format_count(report.metrics.loose_objects),
        format!("{:.2} MiB", to_mib(report.metrics.loose_size_bytes)),
      ],
      vec![
        "Packed objects".to_string(),
        format_count(report.metrics.packed_objects),
        format!("{:.2} MiB", to_mib(report.metrics.packed_size_bytes)),
      ],
    ],
  );
  println!("  Total size: {}", format_size_gib(report.metrics.total_size_bytes));

  print_section("Object inventory");
  let mut inventory_rows = Vec::new();
  for (typ, count) in &report.metrics.object_types {
    inventory_rows.push(vec![typ.clone(), format_count(*count)]);
  }
  print_table(
    &[("Type", CellAlignment::Left), ("Count", CellAlignment::Right)],
    inventory_rows,
  );

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
        vec![
          format!("{}", idx + 1),
          short_oid(&blob.oid).to_string(),
          format!("{:.2} MiB", to_mib(blob.size)),
          blob.path.clone().unwrap_or_default(),
        ]
      })
      .collect();
    print_table(
      &[
        ("#", CellAlignment::Right),
        ("Blob", CellAlignment::Left),
        ("Size", CellAlignment::Right),
        ("Example", CellAlignment::Left),
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
        vec![
          format!("{}", idx + 1),
          short_oid(&tree.oid).to_string(),
          format!("{:.2} KiB", tree.size as f64 / 1024.0),
        ]
      })
      .collect();
    print_table(
      &[("#", CellAlignment::Right), ("Tree", CellAlignment::Left), ("Size", CellAlignment::Right)],
      rows,
    );
  }

  print_section("References");
  print_table(
    &[("Category", CellAlignment::Left), ("Count", CellAlignment::Right)],
    vec![
      vec!["Total".to_string(), format_count(report.metrics.refs_total as u64)],
      vec!["Heads".to_string(), format_count(report.metrics.refs_heads as u64)],
      vec!["Tags".to_string(), format_count(report.metrics.refs_tags as u64)],
      vec!["Remotes".to_string(), format_count(report.metrics.refs_remotes as u64)],
      vec!["Other".to_string(), format_count(report.metrics.refs_other as u64)],
    ],
  );

  print_section("Working tree snapshot (HEAD)");
  let mut snapshot_rows = Vec::new();
  if let Some(dir) = &report.metrics.directory_hotspots {
    snapshot_rows.push(vec![
      "Busiest directory".to_string(),
      dir.path.clone(),
      format!("{} entries", format_count(dir.entries as u64)),
    ]);
  }
  if let Some(path) = &report.metrics.longest_path {
    snapshot_rows.push(vec![
      "Longest path".to_string(),
      path.path.clone(),
      format!("{} characters", format_count(path.length as u64)),
    ]);
  }
  print_table(
    &[("Metric", CellAlignment::Left), ("Value", CellAlignment::Left), ("Details", CellAlignment::Left)],
    snapshot_rows,
  );
  if !report.metrics.duplicate_blobs.is_empty() {
    let shown = report.metrics.duplicate_blobs.len().min(cfg.top);
    println!(
      "  Duplicate blobs (top {}):",
      format_count(shown as u64)
    );
    let rows = report
      .metrics
      .duplicate_blobs
      .iter()
      .enumerate()
      .map(|(idx, dup)| {
        vec![
          format!("{}", idx + 1),
          short_oid(&dup.oid).to_string(),
          format_count(dup.paths as u64),
          dup.example_path.clone().unwrap_or_default(),
        ]
      })
      .collect();
    print_table(
      &[
        ("#", CellAlignment::Right),
        ("Blob", CellAlignment::Left),
        ("Paths", CellAlignment::Right),
        ("Example", CellAlignment::Left),
      ],
      rows,
    );
  }

  print_section("History oddities");
  print_table(
    &[("Metric", CellAlignment::Left), ("Value", CellAlignment::Right)],
    vec![vec!["Max parents".to_string(), format_count(report.metrics.max_commit_parents as u64)]],
  );
  if !report.metrics.oversized_commit_messages.is_empty() {
    println!("  Oversized commit messages:");
    let rows = report
      .metrics
      .oversized_commit_messages
      .iter()
      .enumerate()
      .map(|(idx, msg)| {
        vec![
          format!("{}", idx + 1),
          short_oid(&msg.oid).to_string(),
          format_count(msg.length as u64),
        ]
      })
      .collect();
    print_table(
      &[("#", CellAlignment::Right), ("Commit", CellAlignment::Left), ("Bytes", CellAlignment::Right)],
      rows,
    );
  }

  print_section("Warnings");
  let warning_rows = report
    .warnings
    .iter()
    .map(|warning| {
      vec![
        format!("{:?}", warning.level),
        warning.message.clone(),
        warning.recommendation.clone().unwrap_or_default(),
      ]
    })
    .collect();
  print_table(
    &[("Level", CellAlignment::Center), ("Message", CellAlignment::Left), ("Recommendation", CellAlignment::Left)],
    warning_rows,
  );
}

fn run_git_capture(repo: &Path, args: &[&str]) -> io::Result<String> {
  let out = Command::new("git")
    .current_dir(repo)
    .args(args)
    .stdout(Stdio::piped())
    .stderr(Stdio::inherit())
    .output()?;
  if !out.status.success() {
    return Err(io::Error::new(io::ErrorKind::Other, format!("git {:?} failed", args)));
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
  heap
    .into_sorted_vec()
    .into_iter()
    .map(|Reverse((size, oid))| ObjectStat { oid, size, path: None })
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

fn print_table(headers: &[(&str, CellAlignment)], rows: Vec<Vec<String>>) {
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
      .map(|((_, align), value)| Cell::new(value).set_alignment(*align))
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

fn short_oid(oid: &str) -> &str {
  let end = oid.len().min(12);
  &oid[..end]
}
