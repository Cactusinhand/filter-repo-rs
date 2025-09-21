use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use regex::bytes::Regex;
use serde::Deserialize;

/// Stage-3 toggle: set to `false` to error out instead of accepting legacy cleanup syntax.
const LEGACY_CLEANUP_SYNTAX_ALLOWED: bool = true;
/// Stage-3 toggle: set to `false` to disable legacy --analyze-*-warn overrides entirely.
const LEGACY_ANALYZE_THRESHOLD_FLAGS_ALLOWED: bool = true;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode {
    None,
    Standard,
    Aggressive,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Filter,
    Analyze,
}

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
        Self {
            json: false,
            top: 10,
            thresholds: AnalyzeThresholds::default(),
        }
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
                enforce_legacy_analyze_flag_allowed("--analyze-total-warn", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-total-warn",
                    "analyze.thresholds.warn_total_bytes",
                );
                let v = it.next().expect("--analyze-total-warn requires BYTES");
                let parsed = parse_u64(&v, "--analyze-total-warn");
                opts.analyze.thresholds.warn_total_bytes = parsed;
                overrides.thresholds.warn_total_bytes = Some(parsed);
            }
            "--analyze-total-critical" => {
                enforce_legacy_analyze_flag_allowed("--analyze-total-critical", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-total-critical",
                    "analyze.thresholds.crit_total_bytes",
                );
                let v = it.next().expect("--analyze-total-critical requires BYTES");
                let parsed = parse_u64(&v, "--analyze-total-critical");
                opts.analyze.thresholds.crit_total_bytes = parsed;
                overrides.thresholds.crit_total_bytes = Some(parsed);
            }
            "--analyze-large-blob" => {
                enforce_legacy_analyze_flag_allowed("--analyze-large-blob", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-large-blob",
                    "analyze.thresholds.warn_blob_bytes",
                );
                let v = it.next().expect("--analyze-large-blob requires BYTES");
                let parsed = parse_u64(&v, "--analyze-large-blob");
                opts.analyze.thresholds.warn_blob_bytes = parsed;
                overrides.thresholds.warn_blob_bytes = Some(parsed);
            }
            "--analyze-ref-warn" => {
                enforce_legacy_analyze_flag_allowed("--analyze-ref-warn", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-ref-warn",
                    "analyze.thresholds.warn_ref_count",
                );
                let v = it.next().expect("--analyze-ref-warn requires COUNT");
                let parsed = parse_usize(&v, "--analyze-ref-warn");
                opts.analyze.thresholds.warn_ref_count = parsed;
                overrides.thresholds.warn_ref_count = Some(parsed);
            }
            "--analyze-object-warn" => {
                enforce_legacy_analyze_flag_allowed("--analyze-object-warn", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-object-warn",
                    "analyze.thresholds.warn_object_count",
                );
                let v = it.next().expect("--analyze-object-warn requires COUNT");
                let parsed = parse_usize(&v, "--analyze-object-warn");
                opts.analyze.thresholds.warn_object_count = parsed;
                overrides.thresholds.warn_object_count = Some(parsed);
            }
            "--analyze-tree-entries" => {
                enforce_legacy_analyze_flag_allowed("--analyze-tree-entries", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-tree-entries",
                    "analyze.thresholds.warn_tree_entries",
                );
                let v = it.next().expect("--analyze-tree-entries requires COUNT");
                let parsed = parse_usize(&v, "--analyze-tree-entries");
                opts.analyze.thresholds.warn_tree_entries = parsed;
                overrides.thresholds.warn_tree_entries = Some(parsed);
            }
            "--analyze-path-length" => {
                enforce_legacy_analyze_flag_allowed("--analyze-path-length", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-path-length",
                    "analyze.thresholds.warn_path_length",
                );
                let v = it.next().expect("--analyze-path-length requires LENGTH");
                let parsed = parse_usize(&v, "--analyze-path-length");
                opts.analyze.thresholds.warn_path_length = parsed;
                overrides.thresholds.warn_path_length = Some(parsed);
            }
            "--analyze-duplicate-paths" => {
                enforce_legacy_analyze_flag_allowed("--analyze-duplicate-paths", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-duplicate-paths",
                    "analyze.thresholds.warn_duplicate_paths",
                );
                let v = it.next().expect("--analyze-duplicate-paths requires COUNT");
                let parsed = parse_usize(&v, "--analyze-duplicate-paths");
                opts.analyze.thresholds.warn_duplicate_paths = parsed;
                overrides.thresholds.warn_duplicate_paths = Some(parsed);
            }
            "--analyze-commit-msg-warn" => {
                enforce_legacy_analyze_flag_allowed("--analyze-commit-msg-warn", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-commit-msg-warn",
                    "analyze.thresholds.warn_commit_msg_bytes",
                );
                let v = it.next().expect("--analyze-commit-msg-warn requires BYTES");
                let parsed = parse_usize(&v, "--analyze-commit-msg-warn");
                opts.analyze.thresholds.warn_commit_msg_bytes = parsed;
                overrides.thresholds.warn_commit_msg_bytes = Some(parsed);
            }
            "--analyze-max-parents-warn" => {
                enforce_legacy_analyze_flag_allowed("--analyze-max-parents-warn", opts.debug_mode);
                warn_legacy_analyze_threshold(
                    "--analyze-max-parents-warn",
                    "analyze.thresholds.warn_max_parents",
                );
                let v = it
                    .next()
                    .expect("--analyze-max-parents-warn requires COUNT");
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
                opts.path_renames
                    .push((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
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
                let dir = it
                    .next()
                    .expect("--to-subdirectory-filter requires DIRECTORY");
                let mut d = dir.as_bytes().to_vec();
                if !d.ends_with(b"/") {
                    d.push(b'/');
                }
                opts.path_renames.push((Vec::new(), d));
            }
            "--tag-rename" => {
                let v = it
                    .next()
                    .expect("--tag-rename requires OLD:NEW (either may be empty)");
                let parts: Vec<&str> = v.splitn(2, ':').collect();
                if parts.len() != 2 {
                    eprintln!("--tag-rename expects OLD:NEW");
                    std::process::exit(2);
                }
                opts.tag_rename =
                    Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
            }
            "--branch-rename" => {
                let v = it
                    .next()
                    .expect("--branch-rename requires OLD:NEW (either may be empty)");
                let parts: Vec<&str> = v.splitn(2, ':').collect();
                if parts.len() != 2 {
                    eprintln!("--branch-rename expects OLD:NEW");
                    std::process::exit(2);
                }
                opts.branch_rename =
                    Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
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
                    eprintln!(
                        "error: failed to read config at {}: {}",
                        path.display(),
                        err
                    );
                    std::process::exit(2);
                }
            }
            Err(ConfigError::Parse(err)) => {
                eprintln!(
                    "error: failed to parse config at {}: {}",
                    path.display(),
                    err
                );
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
    enforce_legacy_cleanup_allowed();
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
    if !legacy_warning_once(&format!("cleanup:{mode}")) {
        return;
    }

    match mode {
        "none" => {
            eprintln!(
        "warning: --cleanup=none is deprecated; simply omit --cleanup to keep cleanup disabled."
      );
        }
        "standard" => {
            eprintln!(
        "warning: --cleanup=standard is deprecated; use --cleanup (boolean) to request standard cleanup."
      );
        }
        "aggressive" => {
            eprintln!(
        "warning: --cleanup=aggressive is deprecated; use --cleanup-aggressive in debug mode if you need the old aggressive behavior."
      );
        }
        _ => {
            eprintln!(
        "warning: --cleanup with an explicit value is deprecated; use --cleanup or --cleanup-aggressive instead."
      );
        }
    }
    eprintln!("note: see docs/CLI-CONVERGENCE.zh-CN.md for the cleanup migration guide.");
}

fn legacy_warning_once(key: &str) -> bool {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let warned_set = WARNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut warned = warned_set.lock().expect("Mutex poisoned");
    warned.insert(key.to_string())
}

fn enforce_legacy_cleanup_allowed() {
    if LEGACY_CLEANUP_SYNTAX_ALLOWED {
        return;
    }

    eprintln!("error: legacy --cleanup=<mode> syntax has been removed; use --cleanup or --cleanup-aggressive.");
    std::process::exit(2);
}

fn enforce_legacy_analyze_flag_allowed(flag: &str, debug_mode: bool) {
    if !LEGACY_ANALYZE_THRESHOLD_FLAGS_ALLOWED {
        eprintln!(
      "error: {flag} is no longer accepted; configure analyze.thresholds.* in your filter-repo-rs config file instead."
    );
        std::process::exit(2);
    }
    guard_debug(flag, debug_mode);
}

fn warn_legacy_analyze_threshold(flag: &str, config_key: &str) {
    if !legacy_warning_once(flag) {
        return;
    }

    eprintln!(
    "warning: {flag} is deprecated; set {config_key} in your .filter-repo-rs.toml (or --config) file instead."
  );
    eprintln!(
        "note: see docs/CLI-CONVERGENCE.zh-CN.md for the analysis threshold migration table."
    );
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

#[derive(Debug, Clone)]
struct HelpOption {
    name: String,
    description: Vec<String>,
}

#[derive(Debug, Clone)]
struct HelpSection {
    title: String,
    options: Vec<HelpOption>,
}

fn format_help_option(option: &HelpOption, align_width: usize) -> String {
    let mut result = String::new();
    let indent = "  ";

    if option.description.is_empty() {
        return format!("{}{}", indent, option.name);
    }

    // Handle empty name (description-only lines)
    if option.name.is_empty() {
        for line in &option.description {
            result.push_str(&format!("{}{}\n", indent, line));
        }
        return result;
    }

    let name_padding = " ".repeat(align_width - option.name.len());

    // First line: option name + description
    result.push_str(&format!(
        "{}{}{}{}\n",
        indent, option.name, name_padding, option.description[0]
    ));

    // Subsequent lines: just description with proper indentation
    for line in option.description.iter().skip(1) {
        result.push_str(&format!("{}{}{}\n", indent, " ".repeat(align_width), line));
    }

    result
}

fn format_help_section(section: &HelpSection) -> String {
    if section.options.is_empty() {
        return format!("{}\n", section.title);
    }

    // Calculate the maximum width needed for alignment
    let max_name_width = section
        .options
        .iter()
        .map(|opt| opt.name.len())
        .max()
        .unwrap_or(0);

    // Ensure minimum alignment width for readability
    let align_width = (max_name_width + 2).max(25);

    let mut result = String::new();
    result.push_str(&format!("{}\n", section.title));

    for option in &section.options {
        result.push_str(&format_help_option(option, align_width));
    }

    result.push('\n');
    result
}

fn get_base_help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "Repository & ref selection:".to_string(),
            options: vec![
                HelpOption {
                    name: "--source DIR".to_string(),
                    description: vec!["Source Git working directory (default .)".to_string()],
                },
                HelpOption {
                    name: "--target DIR".to_string(),
                    description: vec!["Target Git working directory (default .)".to_string()],
                },
                HelpOption {
                    name: "--refs REF".to_string(),
                    description: vec!["Ref to export (repeatable; defaults to --all)".to_string()],
                },
                HelpOption {
                    name: "--no-data".to_string(),
                    description: vec!["Do not include blob data in fast-export".to_string()],
                },
            ],
        },
        HelpSection {
            title: "Path selection & rewriting:".to_string(),
            options: vec![
                HelpOption {
                    name: "--path PREFIX".to_string(),
                    description: vec!["Include-only files under PREFIX (repeatable)".to_string()],
                },
                HelpOption {
                    name: "--path-glob GLOB".to_string(),
                    description: vec!["Include by glob (repeatable)".to_string()],
                },
                HelpOption {
                    name: "--path-regex REGEX".to_string(),
                    description: vec!["Include by Rust regex (repeatable)".to_string()],
                },
                HelpOption {
                    name: "--invert-paths".to_string(),
                    description: vec!["Invert path selection (drop matches)".to_string()],
                },
                HelpOption {
                    name: "--path-rename OLD:NEW".to_string(),
                    description: vec!["Rename path prefix in file changes".to_string()],
                },
                HelpOption {
                    name: "--subdirectory-filter D".to_string(),
                    description: vec!["Equivalent to --path D/ --path-rename D/:".to_string()],
                },
                HelpOption {
                    name: "--to-subdirectory-filter D".to_string(),
                    description: vec!["Equivalent to --path-rename :D/".to_string()],
                },
            ],
        },
        HelpSection {
            title: "Blob filtering & redaction:".to_string(),
            options: vec![
                HelpOption {
                    name: "--replace-text FILE".to_string(),
                    description: vec![
                        "Literal/regex (feature-gated) replacements for blobs".to_string()
                    ],
                },
                HelpOption {
                    name: "--max-blob-size BYTES".to_string(),
                    description: vec!["Drop blobs larger than BYTES".to_string()],
                },
                HelpOption {
                    name: "--strip-blobs-with-ids FILE".to_string(),
                    description: vec!["Drop blobs by 40-hex id (one per line)".to_string()],
                },
            ],
        },
        HelpSection {
            title: "Commit, tag & ref updates:".to_string(),
            options: vec![
                HelpOption {
                    name: "--replace-message FILE".to_string(),
                    description: vec!["Literal replacements in commit/tag messages".to_string()],
                },
                HelpOption {
                    name: "--tag-rename OLD:NEW".to_string(),
                    description: vec!["Rename tags with given prefix".to_string()],
                },
                HelpOption {
                    name: "--branch-rename OLD:NEW".to_string(),
                    description: vec!["Rename branches with given prefix".to_string()],
                },
            ],
        },
        HelpSection {
            title: "Execution behavior & output:".to_string(),
            options: vec![
                HelpOption {
                    name: "--write-report".to_string(),
                    description: vec!["Write .git/filter-repo/report.txt summary".to_string()],
                },
                HelpOption {
                    name: "--cleanup".to_string(),
                    description: vec![
                        "Run post-import cleanup (reflog expire + git gc)".to_string(),
                        "(disabled by default)".to_string(),
                    ],
                },
                HelpOption {
                    name: "--quiet".to_string(),
                    description: vec!["Reduce output noise".to_string()],
                },
                HelpOption {
                    name: "--force, -f".to_string(),
                    description: vec![
                        "Bypass safety prompts and checks where applicable".to_string()
                    ],
                },
                HelpOption {
                    name: "--enforce-sanity".to_string(),
                    description: vec!["Fail early unless repo passes strict preflight".to_string()],
                },
                HelpOption {
                    name: "--dry-run".to_string(),
                    description: vec!["Prepare and validate without writing changes".to_string()],
                },
                HelpOption {
                    name: "--partial".to_string(),
                    description: vec!["Only rewrite current repo; skip remote cleanup".to_string()],
                },
                HelpOption {
                    name: "--sensitive".to_string(),
                    description: vec![
                        "Enable sensitive-history mode (fetch all refs,".to_string(),
                        "avoid remote cleanup; see --no-fetch)".to_string(),
                    ],
                },
                HelpOption {
                    name: "--no-fetch".to_string(),
                    description: vec![
                        "In sensitive mode, skip fetching refs from origin".to_string()
                    ],
                },
            ],
        },
        HelpSection {
            title: "Safety & backup:".to_string(),
            options: vec![
                HelpOption {
                    name: "--backup".to_string(),
                    description: vec![
                        "Create a backup bundle of selected refs before".to_string(),
                        "rewriting (skipped with --dry-run)".to_string(),
                    ],
                },
                HelpOption {
                    name: "--backup-path PATH".to_string(),
                    description: vec![
                        "Destination directory or file for the bundle.".to_string(),
                        "If PATH is a directory, a timestamped filename".to_string(),
                        "is generated. If PATH has an extension, that".to_string(),
                        "exact file is written. Defaults to".to_string(),
                        ".git/filter-repo/backup-<timestamp>.bundle".to_string(),
                    ],
                },
            ],
        },
        HelpSection {
            title: "Repository analysis:".to_string(),
            options: vec![
                HelpOption {
                    name: "--analyze".to_string(),
                    description: vec!["Collect repository metrics instead of rewriting".to_string()],
                },
                HelpOption {
                    name: "--analyze-json".to_string(),
                    description: vec!["Emit JSON-formatted analysis report".to_string()],
                },
                HelpOption {
                    name: "--analyze-top N".to_string(),
                    description: vec![
                        "Number of largest blobs/trees to show (default 10)".to_string()
                    ],
                },
            ],
        },
    ]
}

fn get_debug_help_sections() -> Vec<HelpSection> {
    vec![
        HelpSection {
            title: "Debug / fast-export passthrough (require --debug-mode or FRRS_DEBUG=1):"
                .to_string(),
            options: vec![
                HelpOption {
                    name: "--date-order".to_string(),
                    description: vec![
                        "Request date-order traversal from git fast-export".to_string()
                    ],
                },
                HelpOption {
                    name: "--no-reencode".to_string(),
                    description: vec!["Disable re-encoding of commit/tag messages".to_string()],
                },
                HelpOption {
                    name: "--no-quotepath".to_string(),
                    description: vec!["Disable Git's path quoting for non-ASCII".to_string()],
                },
                HelpOption {
                    name: "--no-mark-tags".to_string(),
                    description: vec!["Do not mark annotated tags in fast-export".to_string()],
                },
                HelpOption {
                    name: "--mark-tags".to_string(),
                    description: vec!["Explicitly mark annotated tags in fast-export".to_string()],
                },
            ],
        },
        HelpSection {
            title: "Debug / analysis thresholds (require --debug-mode or FRRS_DEBUG=1):"
                .to_string(),
            options: vec![HelpOption {
                name: "".to_string(), // Empty name for description-only line
                description: vec![
                    "Configure analyze.thresholds.* via .filter-repo-rs.toml or --config."
                        .to_string(),
                    "Legacy --analyze-*-warn CLI flags remain for compatibility but emit warnings."
                        .to_string(),
                ],
            }],
        },
        HelpSection {
            title: "Debug / cleanup behavior (require --debug-mode or FRRS_DEBUG=1):".to_string(),
            options: vec![
                HelpOption {
                    name: "--no-reset".to_string(),
                    description: vec!["Skip final 'git reset --hard' in target".to_string()],
                },
                HelpOption {
                    name: "--cleanup-aggressive".to_string(),
                    description: vec![
                        "Extend cleanup with git gc --aggressive and".to_string(),
                        "--expire-unreachable=now".to_string(),
                    ],
                },
            ],
        },
        HelpSection {
            title: "Debug / stream overrides (require --debug-mode or FRRS_DEBUG=1):".to_string(),
            options: vec![HelpOption {
                name: "--fe_stream_override FILE".to_string(),
                description: vec!["Read fast-export stream from FILE instead of git".to_string()],
            }],
        },
    ]
}

fn get_misc_help_section() -> HelpSection {
    HelpSection {
        title: "Misc:".to_string(),
        options: vec![
            HelpOption {
                name: "--config FILE".to_string(),
                description: vec![
                    "Load options from TOML config file (default".to_string(),
                    "<source>/.filter-repo-rs.toml)".to_string(),
                ],
            },
            HelpOption {
                name: "--debug-mode".to_string(),
                description: vec!["Enable debug/test flags (same as FRRS_DEBUG=1)".to_string()],
            },
            HelpOption {
                name: "-h, --help".to_string(),
                description: vec!["Show this help message".to_string()],
            },
        ],
    }
}

#[allow(dead_code)]
pub fn print_help(debug_mode: bool) {
    println!("filter-repo-rs (prototype)");
    println!("Usage: filter-repo-rs [options]");
    println!();

    // Print base help sections
    for section in get_base_help_sections() {
        print!("{}", format_help_section(&section));
    }

    // Print debug sections if in debug mode
    if debug_mode {
        for section in get_debug_help_sections() {
            print!("{}", format_help_section(&section));
        }
    }

    // Print misc section
    print!("{}", format_help_section(&get_misc_help_section()));
}
