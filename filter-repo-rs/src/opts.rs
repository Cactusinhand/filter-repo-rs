use std::fs;
use std::path::{Path, PathBuf};

use regex::bytes::Regex;
use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode { None, Standard, Aggressive }

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Filter, Analyze }

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AnalyzeThresholds {
  pub warn_total_bytes: u64,
  pub crit_total_bytes: u64,
  pub warn_blob_bytes: u64,
  pub warn_ref_count: usize,
  pub warn_object_count: usize,
  pub warn_tree_entries: usize,
  pub warn_path_length: usize,
  pub warn_duplicate_paths: usize,
  pub warn_commit_msg_bytes: usize,
  pub warn_max_parents: usize,
}

impl Default for AnalyzeThresholds {
  fn default() -> Self {
    Self {
      warn_total_bytes: 1 * 1024 * 1024 * 1024,
      crit_total_bytes: 5 * 1024 * 1024 * 1024,
      warn_blob_bytes: 10 * 1024 * 1024,
      warn_ref_count: 20_000,
      warn_object_count: 10_000_000,
      warn_tree_entries: 2_000,
      warn_path_length: 200,
      warn_duplicate_paths: 1_000,
      warn_commit_msg_bytes: 10_000,
      warn_max_parents: 8,
    }
  }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct AnalyzeConfig {
  pub json: bool,
  pub top: usize,
  pub thresholds: AnalyzeThresholds,
}

impl Default for AnalyzeConfig {
  fn default() -> Self {
    Self { json: false, top: 10, thresholds: AnalyzeThresholds::default() }
  }
}

#[derive(Debug, Default, Deserialize)]
struct FileAnalyzeConfig {
  json: Option<bool>,
  top: Option<usize>,
  thresholds: Option<AnalyzeThresholdOverrides>,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
  analyze: Option<FileAnalyzeConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct AnalyzeThresholdOverrides {
  warn_total_bytes: Option<u64>,
  crit_total_bytes: Option<u64>,
  warn_blob_bytes: Option<u64>,
  warn_ref_count: Option<usize>,
  warn_object_count: Option<usize>,
  warn_tree_entries: Option<usize>,
  warn_path_length: Option<usize>,
  warn_duplicate_paths: Option<usize>,
  warn_commit_msg_bytes: Option<usize>,
  warn_max_parents: Option<usize>,
}

macro_rules! apply_threshold_field {
  ($dest:expr, $src:expr, $field:ident) => {
    if let Some(value) = $src.$field {
      $dest.$field = value;
    }
  };
}

impl AnalyzeThresholdOverrides {
  fn apply(&self, thresholds: &mut AnalyzeThresholds) {
    apply_threshold_field!(thresholds, self, warn_total_bytes);
    apply_threshold_field!(thresholds, self, crit_total_bytes);
    apply_threshold_field!(thresholds, self, warn_blob_bytes);
    apply_threshold_field!(thresholds, self, warn_ref_count);
    apply_threshold_field!(thresholds, self, warn_object_count);
    apply_threshold_field!(thresholds, self, warn_tree_entries);
    apply_threshold_field!(thresholds, self, warn_path_length);
    apply_threshold_field!(thresholds, self, warn_duplicate_paths);
    apply_threshold_field!(thresholds, self, warn_commit_msg_bytes);
    apply_threshold_field!(thresholds, self, warn_max_parents);
  }
}

#[derive(Default)]
struct AnalyzeOverrides {
  json: Option<bool>,
  top: Option<usize>,
  thresholds: AnalyzeThresholdOverrides,
}

impl AnalyzeOverrides {
  fn apply(&self, analyze: &mut AnalyzeConfig) {
    if let Some(json) = self.json {
      analyze.json = json;
    }
    if let Some(top) = self.top {
      analyze.top = top;
    }
    self.thresholds.apply(&mut analyze.thresholds);
  }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Options {
  pub source: PathBuf,
  pub target: PathBuf,
  pub refs: Vec<String>,
  pub date_order: bool,
  pub no_data: bool,
  pub quiet: bool,
  pub reset: bool,
  pub replace_message_file: Option<PathBuf>,
  pub replace_text_file: Option<PathBuf>,
  pub paths: Vec<Vec<u8>>,
  pub invert_paths: bool,
  pub path_globs: Vec<Vec<u8>>,
  pub path_regexes: Vec<Regex>,
  pub path_renames: Vec<(Vec<u8>, Vec<u8>)>,
  pub tag_rename: Option<(Vec<u8>, Vec<u8>)>,
  pub branch_rename: Option<(Vec<u8>, Vec<u8>)>,
  pub max_blob_size: Option<usize>,
  pub strip_blobs_with_ids: Option<PathBuf>,
  pub write_report: bool,
  pub cleanup: CleanupMode,
  pub reencode: bool,
  pub quotepath: bool,
  pub mark_tags: bool,
  pub fe_stream_override: Option<PathBuf>,
  pub force: bool,
  pub enforce_sanity: bool,
  pub dry_run: bool,
  pub partial: bool,
  pub sensitive: bool,
  pub no_fetch: bool,
  pub backup: bool,
  pub backup_path: Option<PathBuf>,
  pub mode: Mode,
  pub analyze: AnalyzeConfig,
  pub debug_mode: bool,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      source: PathBuf::from("."),
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
      path_regexes: Vec::new(),
      path_renames: Vec::new(),
      tag_rename: None,
      branch_rename: None,
      max_blob_size: None,
      strip_blobs_with_ids: None,
      write_report: false,
      cleanup: CleanupMode::None,
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
      backup: false,
      backup_path: None,
      mode: Mode::Filter,
      analyze: AnalyzeConfig::default(),
      debug_mode: false,
    }
  }
}

#[allow(dead_code)]
pub fn parse_args() -> Options {
  use std::env;
  let mut args: Vec<String> = env::args().skip(1).collect();
  let mut config_override = env::var("FILTER_REPO_RS_CONFIG").ok().map(PathBuf::from);

  let mut idx = 0;
  while idx < args.len() {
    if args[idx] == "--config" {
      if idx + 1 >= args.len() {
        eprintln!("error: --config requires a file path");
        std::process::exit(2);
      }
      config_override = Some(PathBuf::from(args.remove(idx + 1)));
      args.remove(idx);
      continue;
    } else if let Some(path) = args[idx].strip_prefix("--config=") {
      if path.is_empty() {
        eprintln!("error: --config= requires a file path");
        std::process::exit(2);
      }
      config_override = Some(PathBuf::from(path));
      args.remove(idx);
      continue;
    }
    idx += 1;
  }

  let mut opts = Options::default();
  opts.debug_mode = debug_mode_enabled(&args);
  let mut overrides = AnalyzeOverrides::default();
  let mut it = args.into_iter();
  while let Some(arg) = it.next() {
    match arg.as_str() {
      "--analyze" => opts.mode = Mode::Analyze,
      "--analyze-json" => {
        opts.analyze.json = true;
        overrides.json = Some(true);
      }
      "--analyze-top" => {
        let v = it.next().expect("--analyze-top requires COUNT");
        let n = parse_usize(&v, "--analyze-top");
        let top = n.max(1);
        opts.analyze.top = top;
        overrides.top = Some(top);
      }
      "--analyze-total-warn" => {
        guard_debug("--analyze-total-warn", opts.debug_mode);
        let v = it.next().expect("--analyze-total-warn requires BYTES");
        let parsed = parse_u64(&v, "--analyze-total-warn");
        opts.analyze.thresholds.warn_total_bytes = parsed;
        overrides.thresholds.warn_total_bytes = Some(parsed);
      }
      "--analyze-total-critical" => {
        guard_debug("--analyze-total-critical", opts.debug_mode);
        let v = it.next().expect("--analyze-total-critical requires BYTES");
        let parsed = parse_u64(&v, "--analyze-total-critical");
        opts.analyze.thresholds.crit_total_bytes = parsed;
        overrides.thresholds.crit_total_bytes = Some(parsed);
      }
      "--analyze-large-blob" => {
        guard_debug("--analyze-large-blob", opts.debug_mode);
        let v = it.next().expect("--analyze-large-blob requires BYTES");
        let parsed = parse_u64(&v, "--analyze-large-blob");
        opts.analyze.thresholds.warn_blob_bytes = parsed;
        overrides.thresholds.warn_blob_bytes = Some(parsed);
      }
      "--analyze-ref-warn" => {
        guard_debug("--analyze-ref-warn", opts.debug_mode);
        let v = it.next().expect("--analyze-ref-warn requires COUNT");
        let parsed = parse_usize(&v, "--analyze-ref-warn");
        opts.analyze.thresholds.warn_ref_count = parsed;
        overrides.thresholds.warn_ref_count = Some(parsed);
      }
      "--analyze-object-warn" => {
        guard_debug("--analyze-object-warn", opts.debug_mode);
        let v = it.next().expect("--analyze-object-warn requires COUNT");
        let parsed = parse_usize(&v, "--analyze-object-warn");
        opts.analyze.thresholds.warn_object_count = parsed;
        overrides.thresholds.warn_object_count = Some(parsed);
      }
      "--analyze-tree-entries" => {
        guard_debug("--analyze-tree-entries", opts.debug_mode);
        let v = it.next().expect("--analyze-tree-entries requires COUNT");
        let parsed = parse_usize(&v, "--analyze-tree-entries");
        opts.analyze.thresholds.warn_tree_entries = parsed;
        overrides.thresholds.warn_tree_entries = Some(parsed);
      }
      "--analyze-path-length" => {
        guard_debug("--analyze-path-length", opts.debug_mode);
        let v = it.next().expect("--analyze-path-length requires LENGTH");
        let parsed = parse_usize(&v, "--analyze-path-length");
        opts.analyze.thresholds.warn_path_length = parsed;
        overrides.thresholds.warn_path_length = Some(parsed);
      }
      "--analyze-duplicate-paths" => {
        guard_debug("--analyze-duplicate-paths", opts.debug_mode);
        let v = it.next().expect("--analyze-duplicate-paths requires COUNT");
        let parsed = parse_usize(&v, "--analyze-duplicate-paths");
        opts.analyze.thresholds.warn_duplicate_paths = parsed;
        overrides.thresholds.warn_duplicate_paths = Some(parsed);
      }
      "--analyze-commit-msg-warn" => {
        guard_debug("--analyze-commit-msg-warn", opts.debug_mode);
        let v = it.next().expect("--analyze-commit-msg-warn requires BYTES");
        let parsed = parse_usize(&v, "--analyze-commit-msg-warn");
        opts.analyze.thresholds.warn_commit_msg_bytes = parsed;
        overrides.thresholds.warn_commit_msg_bytes = Some(parsed);
      }
      "--analyze-max-parents-warn" => {
        guard_debug("--analyze-max-parents-warn", opts.debug_mode);
        let v = it.next().expect("--analyze-max-parents-warn requires COUNT");
        let parsed = parse_usize(&v, "--analyze-max-parents-warn");
        opts.analyze.thresholds.warn_max_parents = parsed;
        overrides.thresholds.warn_max_parents = Some(parsed);
      }
      "--debug-mode" => {
        opts.debug_mode = true;
        continue;
      }
      "--source" => opts.source = PathBuf::from(it.next().expect("--source requires value")),
      "--target" => opts.target = PathBuf::from(it.next().expect("--target requires value")),
      "--ref" | "--refs" => opts.refs.push(it.next().expect("--ref requires value")),
      "--date-order" => {
        guard_debug("--date-order", opts.debug_mode);
        opts.date_order = true;
      }
      "--no-data" => opts.no_data = true,
      "--quiet" => opts.quiet = true,
      "--no-reset" => {
        guard_debug("--no-reset", opts.debug_mode);
        opts.reset = false;
      }
      "--replace-message" => {
        let p = it.next().expect("--replace-message requires file");
        opts.replace_message_file = Some(PathBuf::from(p));
      }
      "--replace-text" => {
        let p = it.next().expect("--replace-text requires file");
        opts.replace_text_file = Some(PathBuf::from(p));
      }
      "--path" => {
        let p = it.next().expect("--path requires value");
        opts.paths.push(p.into_bytes());
      }
      "--invert-paths" => {
        opts.invert_paths = true;
      }
      "--path-glob" => {
        let p = it.next().expect("--path-glob requires value");
        opts.path_globs.push(p.into_bytes());
      }
      "--path-regex" => {
        let p = it.next().expect("--path-regex requires value");
        match Regex::new(&p) {
          Ok(re) => opts.path_regexes.push(re),
          Err(err) => {
            eprintln!("invalid --path-regex '{}': {}", p, err);
            std::process::exit(2);
          }
        }
      }
      "--path-rename" => {
        let v = it.next().expect("--path-rename requires OLD:NEW");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 {
          eprintln!("--path-rename expects OLD:NEW");
          std::process::exit(2);
        }
        opts.path_renames.push((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--subdirectory-filter" => {
        let dir = it.next().expect("--subdirectory-filter requires DIRECTORY");
        let mut d = dir.as_bytes().to_vec();
        if !d.ends_with(b"/") {
          d.push(b'/');
        }
        opts.paths.push(d.clone());
        opts.path_renames.push((d, Vec::new()));
      }
      "--to-subdirectory-filter" => {
        let dir = it.next().expect("--to-subdirectory-filter requires DIRECTORY");
        let mut d = dir.as_bytes().to_vec();
        if !d.ends_with(b"/") {
          d.push(b'/');
        }
        opts.path_renames.push((Vec::new(), d));
      }
      "--tag-rename" => {
        let v = it.next().expect("--tag-rename requires OLD:NEW (either may be empty)");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 {
          eprintln!("--tag-rename expects OLD:NEW");
          std::process::exit(2);
        }
        opts.tag_rename = Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--branch-rename" => {
        let v = it.next().expect("--branch-rename requires OLD:NEW (either may be empty)");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 {
          eprintln!("--branch-rename expects OLD:NEW");
          std::process::exit(2);
        }
        opts.branch_rename = Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--max-blob-size" => {
        let v = it.next().expect("--max-blob-size requires BYTES");
        let n = v.parse::<usize>().unwrap_or_else(|_| {
          eprintln!("--max-blob-size expects an integer number of bytes");
          std::process::exit(2);
        });
        opts.max_blob_size = Some(n);
      }
      "--strip-blobs-with-ids" => {
        let p = it.next().expect("--strip-blobs-with-ids requires FILE");
        opts.strip_blobs_with_ids = Some(PathBuf::from(p));
      }
      "--write-report" => {
        opts.write_report = true;
      }
      "--cleanup" => {
        if let Some(next) = it.clone().next() {
          if matches!(next.as_str(), "none" | "standard" | "aggressive") {
            let legacy = it.next().expect("--cleanup legacy value consumed");
            parse_legacy_cleanup_value(&legacy, &mut opts);
            continue;
          }
        }
        opts.cleanup = CleanupMode::Standard;
      }
      arg if arg.starts_with("--cleanup=") => {
        let value = &arg[10..];
        if value.is_empty() {
          eprintln!("--cleanup= requires a value of none|standard|aggressive");
          std::process::exit(2);
        }
        parse_legacy_cleanup_value(value, &mut opts);
      }
      "--cleanup-aggressive" => {
        guard_debug("--cleanup-aggressive", opts.debug_mode);
        opts.cleanup = CleanupMode::Aggressive;
      }
      "--no-reencode" => {
        guard_debug("--no-reencode", opts.debug_mode);
        opts.reencode = false;
      }
      "--no-quotepath" => {
        guard_debug("--no-quotepath", opts.debug_mode);
        opts.quotepath = false;
      }
      "--no-mark-tags" => {
        guard_debug("--no-mark-tags", opts.debug_mode);
        opts.mark_tags = false;
      }
      "--mark-tags" => {
        guard_debug("--mark-tags", opts.debug_mode);
        opts.mark_tags = true;
      }
      "--force" | "-f" => {
        opts.force = true;
      }
      "--enforce-sanity" => {
        opts.enforce_sanity = true;
      }
      "--dry-run" => {
        opts.dry_run = true;
      }
      "--partial" => {
        opts.partial = true;
      }
      "--sensitive" | "--sensitive-data-removal" => {
        opts.sensitive = true;
      }
      "--no-fetch" => {
        opts.no_fetch = true;
      }
      "--backup" => {
        opts.backup = true;
      }
      "--backup-path" => {
        if let Some(p) = it.next() {
          opts.backup_path = Some(PathBuf::from(p));
        } else {
          eprintln!("error: --backup-path requires a value");
          std::process::exit(2);
        }
      }
      "--fe_stream_override" => {
        guard_debug("--fe_stream_override", opts.debug_mode);
        let p = it.next().expect("--fe_stream_override requires FILE");
        opts.fe_stream_override = Some(PathBuf::from(p));
      }
      "-h" | "--help" => {
        print_help(opts.debug_mode);
        std::process::exit(0);
      }
      other => {
        eprintln!("Unknown argument: {}", other);
        print_help(opts.debug_mode);
        std::process::exit(2);
      }
    }
  }

  let config_target = if let Some(path) = config_override {
    Some((path, true))
  } else {
    Some((opts.source.join(".filter-repo-rs.toml"), false))
  };

  if let Some((path, explicit)) = config_target {
    match apply_config_from_file(&mut opts, &path) {
      Ok(()) => {}
      Err(ConfigError::Io(err)) => {
        use std::io::ErrorKind;
        if explicit || err.kind() != ErrorKind::NotFound {
          eprintln!("error: failed to read config at {}: {}", path.display(), err);
          std::process::exit(2);
        }
      }
      Err(ConfigError::Parse(err)) => {
        eprintln!("error: failed to parse config at {}: {}", path.display(), err);
        eprintln!(
          "note: example key: analyze.thresholds.warn_total_bytes (see docs/cli-convergence.md)"
        );
        std::process::exit(2);
      }
    }
  }

  overrides.apply(&mut opts.analyze);
  opts
}

enum ConfigError {
  Io(std::io::Error),
  Parse(toml::de::Error),
}

fn apply_config_from_file(opts: &mut Options, path: &Path) -> Result<(), ConfigError> {
  let raw = fs::read_to_string(path).map_err(ConfigError::Io)?;
  let config: FileConfig = toml::from_str(&raw).map_err(ConfigError::Parse)?;

  if let Some(analyze) = config.analyze {
    if let Some(json) = analyze.json {
      opts.analyze.json = json;
    }
    if let Some(top) = analyze.top {
      opts.analyze.top = top.max(1);
    }
    if let Some(thresholds) = analyze.thresholds {
      guard_debug("analyze.thresholds.*", opts.debug_mode);
      thresholds.apply(&mut opts.analyze.thresholds);
    }
  }

  Ok(())
}

fn parse_legacy_cleanup_value(value: &str, opts: &mut Options) {
  warn_legacy_cleanup_usage(value);
  opts.cleanup = match value {
    "none" => CleanupMode::None,
    "standard" => CleanupMode::Standard,
    "aggressive" => {
      guard_debug("--cleanup aggressive", opts.debug_mode);
      CleanupMode::Aggressive
    }
    other => {
      eprintln!("--cleanup: unknown mode '{}'", other);
      std::process::exit(2);
    }
  };
}

fn warn_legacy_cleanup_usage(mode: &str) {
  use std::collections::HashSet;
  use std::sync::{Mutex, OnceLock};

  static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
  let mut warned = WARNED.get_or_init(|| Mutex::new(HashSet::new())).lock().expect("Mutex poisoned");
  let key = format!("cleanup:{mode}");
  if !warned.insert(key) {
    return;
  }

  let recommendation = match mode {
    "none" => "omit --cleanup entirely (cleanup defaults to 'none').",
    "standard" => "use --cleanup (without a value) to enable standard cleanup.",
    "aggressive" => "use --cleanup-aggressive (requires --debug-mode or FRRS_DEBUG=1).",
    _ => "use --cleanup or --cleanup-aggressive instead.",
  };

  eprintln!(
    "warning: --cleanup with an explicit value is deprecated; {}",
    recommendation
  );
  eprintln!("note: pass --cleanup as a boolean flag for standard cleanup.");
}

fn debug_mode_enabled(args: &[String]) -> bool {
  use std::env;
  if matches!(env::var("FRRS_DEBUG"), Ok(val) if debug_env_flag_enabled(&val)) {
    return true;
  }
  args.iter().any(|arg| arg == "--debug-mode")
}

fn debug_env_flag_enabled(raw: &str) -> bool {
  let normalized = raw.trim().to_ascii_lowercase();
  if normalized.is_empty() {
    return false;
  }
  !matches!(normalized.as_str(), "0" | "false" | "no" | "off")
}

fn guard_debug(flag: &str, debug_mode: bool) {
  if !debug_mode {
    eprintln!(
      "error: {flag} is gated behind debug mode. Set FRRS_DEBUG=1 or pass --debug-mode to access debug-only flags."
    );
    eprintln!("See docs/cli-convergence.md for the configuration migration plan.");
    std::process::exit(2);
  }
}

fn parse_u64(s: &str, flag: &str) -> u64 {
  s.parse::<u64>().unwrap_or_else(|_| {
    eprintln!("{} expects an integer number", flag);
    std::process::exit(2);
  })
}

fn parse_usize(s: &str, flag: &str) -> usize {
  s.parse::<usize>().unwrap_or_else(|_| {
    eprintln!("{} expects an integer number", flag);
    std::process::exit(2);
  })
}

const BASE_HELP: &str = "filter-repo-rs (prototype)\n\
Usage: filter-repo-rs [options]\n\
\n\
Repository & ref selection:\n\
  --source DIR                Source Git working directory (default .)\n\
  --target DIR                Target Git working directory (default .)\n\
  --refs REF                  Ref to export (repeatable; defaults to --all)\n\
  --no-data                   Do not include blob data in fast-export\n\
\n\
Path selection & rewriting:\n\
  --path PREFIX               Include-only files under PREFIX (repeatable)\n\
  --path-glob GLOB            Include by glob (repeatable)\n\
  --path-regex REGEX          Include by Rust regex (repeatable)\n\
  --invert-paths              Invert path selection (drop matches)\n\
  --path-rename OLD:NEW       Rename path prefix in file changes\n\
  --subdirectory-filter D     Equivalent to --path D/ --path-rename D/:\n\
  --to-subdirectory-filter D  Equivalent to --path-rename :D/\n\
\n\
Blob filtering & redaction:\n\
  --replace-text FILE         Literal/regex (feature-gated) replacements for blobs\n\
  --max-blob-size BYTES       Drop blobs larger than BYTES\n\
  --strip-blobs-with-ids FILE Drop blobs by 40-hex id (one per line)\n\
\n\
Commit, tag & ref updates:\n\
  --replace-message FILE      Literal replacements in commit/tag messages\n\
  --tag-rename OLD:NEW        Rename tags with given prefix\n\
  --branch-rename OLD:NEW     Rename branches with given prefix\n\
\n\
Execution behavior & output:\n\
  --write-report              Write .git/filter-repo/report.txt summary\n\
  --cleanup                   Run post-import cleanup (reflog expire + git gc)\n\
                              (disabled by default)\n\
  --quiet                     Reduce output noise\n\
  --force, -f                 Bypass safety prompts and checks where applicable\n\
  --enforce-sanity            Fail early unless repo passes strict preflight\n\
  --dry-run                   Prepare and validate without writing changes\n\
  --partial                   Only rewrite current repo; skip remote cleanup\n\
  --sensitive, --sensitive-data-removal\n\
                              Enable sensitive-history mode (fetch all refs,\n\
                              avoid remote cleanup; see --no-fetch)\n\
  --no-fetch                  In sensitive mode, skip fetching refs from origin\n\
\n\
Safety & backup:\n\
  --backup                    Create a backup bundle of selected refs before\n\
                              rewriting (skipped with --dry-run)\n\
  --backup-path PATH          Destination directory or file for the bundle.\n\
                              If PATH is a directory, a timestamped filename\n\
                              is generated. If PATH has an extension, that\n\
                              exact file is written. Defaults to\n\
                              .git/filter-repo/backup-<timestamp>.bundle\n\
\n\
Repository analysis:\n\
  --analyze                   Collect repository metrics instead of rewriting\n\
  --analyze-json              Emit JSON-formatted analysis report\n\
  --analyze-top N             Number of largest blobs/trees to show (default 10)\n\
";

const DEBUG_FAST_EXPORT_HELP: &str = "\n\
Debug / fast-export passthrough (require --debug-mode or FRRS_DEBUG=1):\n\
  --date-order                Request date-order traversal from git fast-export\n\
  --no-reencode               Disable re-encoding of commit/tag messages\n\
  --no-quotepath              Disable Git's path quoting for non-ASCII\n\
  --no-mark-tags              Do not mark annotated tags in fast-export\n\
  --mark-tags                 Explicitly mark annotated tags in fast-export\n\
";

const DEBUG_ANALYSIS_HELP: &str = "\n\
Debug / analysis thresholds (require --debug-mode or FRRS_DEBUG=1):\n\
  --analyze-total-warn BYTES  Override warning threshold for total repo size\n\
  --analyze-total-critical BYTES Override critical threshold for total repo size\n\
  --analyze-large-blob BYTES  Override blob size warning threshold\n\
  --analyze-ref-warn COUNT    Override reference count warning threshold\n\
  --analyze-object-warn COUNT Override object count warning threshold\n\
  --analyze-tree-entries N    Override tree entry warning threshold\n\
  --analyze-path-length N     Override path length warning threshold\n\
  --analyze-duplicate-paths N Override duplicate-path warning threshold\n\
  --analyze-commit-msg-warn N Override commit message length warning threshold\n\
  --analyze-max-parents-warn N Override max parent count warning threshold\n\
";

const DEBUG_CLEANUP_HELP: &str = "\n\
Debug / cleanup behavior (require --debug-mode or FRRS_DEBUG=1):\n\
  --no-reset                  Skip final 'git reset --hard' in target\n\
  --cleanup-aggressive        Extend cleanup with git gc --aggressive and\n\
                              --expire-unreachable=now\n\
";

const DEBUG_STREAM_HELP: &str = "\n\
Debug / stream overrides (require --debug-mode or FRRS_DEBUG=1):\n\
  --fe_stream_override FILE   Read fast-export stream from FILE instead of git\n\
";

const MISC_HELP: &str = "\n\
Misc:\n\
  --config FILE              Load options from TOML config file (default\n\
                             <source>/.filter-repo-rs.toml)\n\
  --debug-mode               Enable debug/test flags (same as FRRS_DEBUG=1)\n\
  -h, --help                 Show this help message\n\
";

#[allow(dead_code)]
pub fn print_help(debug_mode: bool) {
  print!("{}", BASE_HELP);
  if debug_mode {
    print!("{}", DEBUG_FAST_EXPORT_HELP);
    print!("{}", DEBUG_ANALYSIS_HELP);
    print!("{}", DEBUG_CLEANUP_HELP);
    print!("{}", DEBUG_STREAM_HELP);
  }
  print!("{}", MISC_HELP);
}
