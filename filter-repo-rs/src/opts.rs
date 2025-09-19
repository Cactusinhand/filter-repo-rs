use std::path::PathBuf;

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
    }
  }
}

#[allow(dead_code)]
pub fn parse_args() -> Options {
  use std::env;
  let mut opts = Options::default();
  let mut it = env::args().skip(1);
  while let Some(arg) = it.next() {
    match arg.as_str() {
      "--analyze" => opts.mode = Mode::Analyze,
      "--analyze-json" => opts.analyze.json = true,
      "--analyze-top" => {
        let v = it.next().expect("--analyze-top requires COUNT");
        let n = parse_usize(&v, "--analyze-top");
        opts.analyze.top = n.max(1);
      }
      "--analyze-total-warn" => {
        let v = it.next().expect("--analyze-total-warn requires BYTES");
        opts.analyze.thresholds.warn_total_bytes = parse_u64(&v, "--analyze-total-warn");
      }
      "--analyze-total-critical" => {
        let v = it.next().expect("--analyze-total-critical requires BYTES");
        opts.analyze.thresholds.crit_total_bytes = parse_u64(&v, "--analyze-total-critical");
      }
      "--analyze-large-blob" => {
        let v = it.next().expect("--analyze-large-blob requires BYTES");
        opts.analyze.thresholds.warn_blob_bytes = parse_u64(&v, "--analyze-large-blob");
      }
      "--analyze-ref-warn" => {
        let v = it.next().expect("--analyze-ref-warn requires COUNT");
        opts.analyze.thresholds.warn_ref_count = parse_usize(&v, "--analyze-ref-warn");
      }
      "--analyze-object-warn" => {
        let v = it.next().expect("--analyze-object-warn requires COUNT");
        opts.analyze.thresholds.warn_object_count = parse_usize(&v, "--analyze-object-warn");
      }
      "--analyze-tree-entries" => {
        let v = it.next().expect("--analyze-tree-entries requires COUNT");
        opts.analyze.thresholds.warn_tree_entries = parse_usize(&v, "--analyze-tree-entries");
      }
      "--analyze-path-length" => {
        let v = it.next().expect("--analyze-path-length requires LENGTH");
        opts.analyze.thresholds.warn_path_length = parse_usize(&v, "--analyze-path-length");
      }
      "--analyze-duplicate-paths" => {
        let v = it.next().expect("--analyze-duplicate-paths requires COUNT");
        opts.analyze.thresholds.warn_duplicate_paths = parse_usize(&v, "--analyze-duplicate-paths");
      }
      "--analyze-commit-msg-warn" => {
        let v = it.next().expect("--analyze-commit-msg-warn requires BYTES");
        opts.analyze.thresholds.warn_commit_msg_bytes = parse_usize(&v, "--analyze-commit-msg-warn");
      }
      "--analyze-max-parents-warn" => {
        let v = it.next().expect("--analyze-max-parents-warn requires COUNT");
        opts.analyze.thresholds.warn_max_parents = parse_usize(&v, "--analyze-max-parents-warn");
      }
      "--source" => opts.source = PathBuf::from(it.next().expect("--source requires value")),
      "--target" => opts.target = PathBuf::from(it.next().expect("--target requires value")),
      "--ref" | "--refs" => opts.refs.push(it.next().expect("--ref requires value")),
      "--date-order" => opts.date_order = true,
      "--no-data" => opts.no_data = true,
      "--quiet" => opts.quiet = true,
      "--no-reset" => opts.reset = false,
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
        let v = it.next().expect("--cleanup requires one of: none|standard|aggressive");
        opts.cleanup = match v.as_str() {
          "none" => CleanupMode::None,
          "standard" => CleanupMode::Standard,
          "aggressive" => CleanupMode::Aggressive,
          other => {
            eprintln!("--cleanup: unknown mode '{}'", other);
            std::process::exit(2);
          }
        };
      }
      "--no-reencode" => {
        opts.reencode = false;
      }
      "--no-quotepath" => {
        opts.quotepath = false;
      }
      "--no-mark-tags" => {
        opts.mark_tags = false;
      }
      "--mark-tags" => {
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
      "-h" | "--help" => {
        print_help();
        std::process::exit(0);
      }
      other => {
        eprintln!("Unknown argument: {}", other);
        print_help();
        std::process::exit(2);
      }
    }
  }
  opts
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

#[allow(dead_code)]
pub fn print_help() {
  println!(
    "filter-repo-rs (prototype)\n\
Usage: filter-repo-rs [options]\n\
\n\
Repository & ref selection:\n\
  --source DIR                Source Git working directory (default .)\n\
  --target DIR                Target Git working directory (default .)\n\
  --refs REF                  Ref to export (repeatable; defaults to --all)\n\
  --date-order                Use date-order for fast-export\n\
  --no-data                   Do not include blob data in fast-export\n\
\n\
Path selection & rewriting:\n\
  --path PREFIX               Include-only files under PREFIX (repeatable)\n\
  --path-glob GLOB            Include by glob (repeatable)\n\
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
  --cleanup MODE              none|standard|aggressive (default: none)\n\
  --quiet                     Reduce output noise\n\
  --no-reset                  Skip final 'git reset --hard' in target\n\
  --no-reencode               Do not re-encode commit/tag messages\n\
  --no-quotepath              Disable Git's path quoting for non-ASCII\n\
  --no-mark-tags              Do not mark annotated tags in fast-export\n\
  --mark-tags                 Mark annotated tags in fast-export (default)\n\
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
\n\
Misc:\n\
  -h, --help                  Show this help message\n\
"
  );
}
