use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode { None, Standard, Aggressive }

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
      "--source" => opts.source = PathBuf::from(it.next().expect("--source requires value")),
      "--target" => opts.target = PathBuf::from(it.next().expect("--target requires value")),
      "--ref" | "--refs" => opts.refs.push(it.next().expect("--ref requires value")),
      "--date-order" => opts.date_order = true,
      "--no-data" => opts.no_data = true,
      "--quiet" => opts.quiet = true,
      "--no-reset" => opts.reset = false,
      "--replace-message" => { let p = it.next().expect("--replace-message requires file"); opts.replace_message_file = Some(PathBuf::from(p)); }
      "--replace-text" => { let p = it.next().expect("--replace-text requires file"); opts.replace_text_file = Some(PathBuf::from(p)); }
      "--path" => { let p = it.next().expect("--path requires value"); opts.paths.push(p.into_bytes()); }
      "--invert-paths" => { opts.invert_paths = true; }
      "--path-glob" => { let p = it.next().expect("--path-glob requires value"); opts.path_globs.push(p.into_bytes()); }
      "--path-rename" => {
        let v = it.next().expect("--path-rename requires OLD:NEW");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 { eprintln!("--path-rename expects OLD:NEW"); std::process::exit(2) }
        opts.path_renames.push((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--subdirectory-filter" => {
        let dir = it.next().expect("--subdirectory-filter requires DIRECTORY");
        let mut d = dir.as_bytes().to_vec(); if !d.ends_with(b"/") { d.push(b'/'); }
        opts.paths.push(d.clone()); opts.path_renames.push((d, Vec::new()));
      }
      "--to-subdirectory-filter" => {
        let dir = it.next().expect("--to-subdirectory-filter requires DIRECTORY");
        let mut d = dir.as_bytes().to_vec(); if !d.ends_with(b"/") { d.push(b'/'); }
        opts.path_renames.push((Vec::new(), d));
      }
      "--tag-rename" => {
        let v = it.next().expect("--tag-rename requires OLD:NEW (either may be empty)");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 { eprintln!("--tag-rename expects OLD:NEW"); std::process::exit(2) }
        opts.tag_rename = Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--branch-rename" => {
        let v = it.next().expect("--branch-rename requires OLD:NEW (either may be empty)");
        let parts: Vec<&str> = v.splitn(2, ':').collect();
        if parts.len() != 2 { eprintln!("--branch-rename expects OLD:NEW"); std::process::exit(2) }
        opts.branch_rename = Some((parts[0].as_bytes().to_vec(), parts[1].as_bytes().to_vec()));
      }
      "--max-blob-size" => {
        let v = it.next().expect("--max-blob-size requires BYTES");
        let n = v.parse::<usize>().unwrap_or_else(|_| { eprintln!("--max-blob-size expects an integer number of bytes"); std::process::exit(2) });
        opts.max_blob_size = Some(n);
      }
      "--strip-blobs-with-ids" => { let p = it.next().expect("--strip-blobs-with-ids requires FILE"); opts.strip_blobs_with_ids = Some(PathBuf::from(p)); }
      "--write-report" => { opts.write_report = true; }
      "--cleanup" => {
        let v = it.next().expect("--cleanup requires one of: none|standard|aggressive");
        opts.cleanup = match v.as_str() { "none" => CleanupMode::None, "standard" => CleanupMode::Standard, "aggressive" => CleanupMode::Aggressive, other => { eprintln!("--cleanup: unknown mode '{}'", other); std::process::exit(2) } };
      }
      "--no-reencode" => { opts.reencode = false; }
      "--no-quotepath" => { opts.quotepath = false; }
      "--no-mark-tags" => { opts.mark_tags = false; }
      "--mark-tags" => { opts.mark_tags = true; }
      "--force" | "-f" => { opts.force = true; }
      "--enforce-sanity" => { opts.enforce_sanity = true; }
      "--dry-run" => { opts.dry_run = true; }
      "--partial" => { opts.partial = true; }
      "--sensitive" | "--sensitive-data-removal" => { opts.sensitive = true; }
      "--no-fetch" => { opts.no_fetch = true; }
      "-h" | "--help" => { print_help(); std::process::exit(0); }
      other => { eprintln!("Unknown argument: {}", other); print_help(); std::process::exit(2); }
    }
  }
  opts
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
  --replace-text FILE          Literal/regex (feature-gated) replacements for blobs\n\
  --max-blob-size BYTES        Drop blobs larger than BYTES\n\
  --strip-blobs-with-ids FILE  Drop blobs by 40-hex id (one per line)\n\
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
  --no-reencode               Do not pass --reencode=yes to fast-export\n\
  --no-quotepath              Do not force core.quotepath=false for export\n\
  --no-mark-tags              Do not pass --mark-tags to fast-export\n\
\n\
Safety & advanced modes:\n\
  --force, -f                 Bypass sanity checks (danger: destructive)\n\
  --enforce-sanity            Enable preflight safety checks\n\
  --dry-run                   Do not update refs or clean up; preview only\n\
  --partial                   Do a partial rewrite; disable origin migration, ref cleanup, reflog gc\n\
  --sensitive                 Sensitive-data mode (enables fetch-all unless --no-fetch)\n\
  --no-fetch                  Do not fetch all refs even in --sensitive mode\n\
  -h, --help                  Show this help\n"
  );
}
