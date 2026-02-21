# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2026-02-21

### Fixed

- Updated README with `cargo install filter-repo-rs` installation instructions

## [1.0.0] - 2026-02-21

> **Stable Release**: The first stable release of filter-repo-rs, a fast and safe Git history rewriting tool written in Rust.

### Highlights

- **Production Ready**: After beta testing, the core functionality is now stable and ready for production use
- **Fast & Safe**: Rust implementation provides memory safety and performance
- **Feature Complete**: All core git-filter-repo features implemented
- **Cross Platform**: Supports Linux, macOS, and Windows

### Changes from 1.0.0-beta.1

- Documentation improvements with README packaging
- Version bump to stable 1.0.0

### Known Limitations

- Merge simplification still being optimized for complex topologies
- No incremental processing (`--state-branch`) yet
- `--path-compat-policy` currently applies only when running on Windows hosts

## [1.0.0-beta] - 2026-02-21

> **Beta Release**: This release marks the transition from prototype to beta. The core functionality is now feature-complete and ready for broader testing.

### Highlights

- **Secret Detection**: New `--detect-secrets` mode to scan and detect potential secrets in Git history
- **Identity Rewriting**: Full support for author/committer/email rewriting via `--mailmap` and individual rewrite files
- **Performance**: Streaming blob processing, Aho-Corasick multi-rule replacement, and parallel processing support
- **Windows Compatibility**: Configurable path compatibility policy and proper Windows path handling
- **Safety**: Comprehensive sanity checks, backup support, and dry-run capabilities

### Known Limitations

- Merge simplification still being optimized for complex topologies
- No incremental processing (`--state-branch`) yet
- `--path-compat-policy` currently applies only when running on Windows hosts

### 🚀 Features

- *(analysis)* Add stream-based git command runner
- Add --write-report-json for machine-readable output
- Add run_git_with_timeout helper for git commands
- *(analysis)* Add terminal color support with TTY detection
- *(analysis)* Group blobs by file path in analyze output
- *(analysis)* Show truncated OIDs directly in tables
- *(analysis)* Add elapsed time and rate to progress indicators
- Add author identity rewrite options
- Add --version/-V flag and bump version to 0.2.0
- *(report)* Improve JSON structure with nested sections
- Highlight git commands in error message with color
- Add timestamp modification options (--date-shift and --date-set)
- Enhance all error messages with colored highlights
- *(detect)* Add --detect-secrets mode to scan potential secrets in history
- *(detect)* Enhance secret detection with custom and LLM patterns
- *(cli)* Standardize --help output format
- *(path-compat)* Add configurable Windows path compatibility policy
- *(cli)* Add runtime stage-3 toggles for legacy flag removal

### 🐛 Bug Fixes

- Fix clippy issue
- Fix clippy issue
- Replace .expect() with proper error handling in stream.rs
- Add proper error handling for git processes in migrate.rs
- Add MAX_BLOB_SIZE constant to prevent memory exhaustion
- Replace process::exit with proper error propagation in finalize.rs
- *(core)* Guard all data blocks and relax origin cleanup failure
- Add warnings when --sensitive skips git remote/origin check
- Add warning log for blob size parsing failure in query_size_via_batch
- *(perf)* Correct dry-run test to run in-place
- *(message)* Keep replace-text byte-safe in Aho-Corasick paths
- *(stream)* Report modified blobs only when payload changes
- Make replace-text single-pass and deterministic
- *(report)* Fix counting logic and add more statistics
- *(gitutil)* Accept linked worktree git-dir structure
- *(analysis)* Stream cat-file object inventory
- *(analysis)* Avoid broken-pipe panics in progress output
- *(opts)* Return invalid-options errors for missing flag values
- *(filechange)* Parse CRLF-terminated filechange lines
- *(analyze)* Keep --analyze-json stdout machine-readable
- *(analyze)* Remove placeholder blob ids from metrics
- *(analyze)* Stream oversized commit message scanning

### 💼 Other

- Cargo fmt
- Update README
- Add rayon and crossbeam for parallel blob processing

- Add rayon and crossbeam dependencies for parallel processing
- Add process_blob_batch_parallel for parallel content replacement
- Add process_blob_content for single blob processing
- Infrastructure ready for batch blob processing optimization
- Remove unused imports and fix warnings
- Remove unused dead code
- Fix cargo clippy errors
- Update .gitignore
- Fix cargo clippy errors
- Fix cargo fmr errors
- Add mailmap docs to README
- Add Co-authored-by tip
- Fix cargo clippy issue
- Fix clippy errors

### 🚜 Refactor

- Make parse_args return Result for proper error handling
- *(analysis)* Remove dead code after streaming refactor
- *(analysis)* Clean up dead code and warnings
- *(report)* Extract add_sample helper function

### 📚 Documentation

- Add performance note to MessageReplacer::apply
- *(testing)* Add testing policy and PR checklist
- *(StripShaLookup)* Document memory optimization opportunities

### ⚡ Performance

- *(analysis)* Use streaming for blob path mapping
- *(analysis)* Use streaming for gather_max_parents
- *(analysis)* Single-pass streaming for commit history
- *(stream)* Lower STRIP_SHA_ON_DISK_THRESHOLD to 50k
- *(message)* Use Aho-Corasick for O(n) multi-rule replacement
- Add streaming blob processing for large files
- Add regex DFA size limits for better memory control
- Reduce memory clones in blob content replacement
- Lower SHA on-disk threshold to reduce memory spike

### 🎨 Styling

- *(test)* Format bdd cli scenario assertions

### 🧪 Testing

- *(common)* Retry transient git spawn failures
- *(bdd)* Add main CLI behavior scenarios
- *(coverage)* Add targeted tests for core runtime paths
- *(perf)* Add performance benchmarking scripts
- *(perf)* Add comprehensive performance testing framework
- *(config)* Update analyze output assertions
- *(identity)* Cover mailmap rewrite behavior and precedence
- *(path-compat)* Run policy coverage on all platforms
- *(rename)* Cover mixed annotated and lightweight tag rename
- *(paths)* Add path filter/rename interaction matrix cases
- *(stream)* Cover escaped quoted-path roundtrip and status notes
- *(analyze)* Lock reachable-only blob metrics semantics
- *(stream)* Make escaped path requote assertions windows-aware

### ⚙️ Miscellaneous Tasks

- Add rustfmt/clippy and fix CI YAML layout
- *(testing)* Split gates and isolate integration suites
## [0.1.2] - 2025-10-03

### 💼 Other

- Update
## [0.1.1] - 2025-10-03

### 💼 Other

- Update
## [0.1.0] - 2025-10-03

### 🚀 Features

- *(messages)* Add regex support for --replace-message
- *(build)* 跨平台构建与发布产物

### 🐛 Bug Fixes

- Fix
- Fix tests on Windows
- Correct freshly packed check logic
- Decode fast-export paths before sanitizing

### 💼 Other

- Init
- Make regex replacements always available
- Merge pull request #1 from Cactusinhand/codex/add-regex-crate-and-update-documentation

Make regex replacements always available
- Stream: stream strip id lookup
- Merge pull request #2 from Cactusinhand/codex/replace-hashset-load-with-iterator

Stream strip-blobs lookup lazily
- More tests
- Update filter-repo-rs/tests/integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Merge pull request #3 from Cactusinhand/test/test

add more tests
- Drop python blob rewrite and update arch doc
- Update
- Normalize CRLF trimming indentation
- Add CI workflow to run cargo tests
- Merge pull request #5 from Cactusinhand/codex/add-github-ci-action-for-cargo-test

Add CI workflow to run cargo tests
- Add macOS and Windows to CI matrix
- Merge pull request #7 from Cactusinhand/codex/add-github-ci-action-for-windows-and-macos

Add macOS and Windows to CI matrix
- Track blob sizes by hex and binary SHA
- Merge pull request #6 from Cactusinhand/codex/fix-test-failure-on-ubuntu

Add binary SHA tracking for blob size lookup
- Merge branch 'main' into codex/refactor-get_blob_sizes-for-streaming-iterator
- Configure blob tracker test repo
- Merge pull request #4 from Cactusinhand/codex/refactor-get_blob_sizes-for-streaming-iterator

filter-repo: document blob tracker without python changes
- Reorganize option documentation by feature
- Update filter-repo-rs/src/opts.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/src/opts.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Update
- Update
- Merge pull request #9 from Cactusinhand/codex/rearrange-options-by-functionality-and-update-readme-c5svvq

Reorganize option documentation by feature
- Add bundle backup support
- Back up before ref migration
- Apply suggestion from @gemini-code-assist[bot]

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Document bundle restore and rename backups
- Merge pull request #10 from Cactusinhand/codex/add-backup-functionality-to-filter-repo-rs

filter-repo-rs: add bundle backup support
- Add repository analysis mode
- Handle large repositories in analysis mode
- Add integration coverage for analysis limits and warnings
- Refine analysis report presentation
- Improve analyze report formatting with comfy-table
- Update crate version
- Merge pull request #11 from Cactusinhand/codex/plan-tool-to-analyze-git-repository-sizes

Add repository analysis mode
- Fix rename quoting test literals
- Merge pull request #12 from Cactusinhand/codex/extract-common-parser-for-fast-export-commands

filter-repo-rs: fix rename quoting test literals
- Batch finalize ref updates
- Merge pull request #13 from Cactusinhand/codex/track-new-mark/oid-on-reset

filter-repo-rs: batch finalize ref updates
- Record null targets for pruned commits
- Merge pull request #14 from Cactusinhand/codex/collect-old-none-mapping-in-process_commit_line

Record null targets for pruned commits
- Update
- Add integration coverage for path maps and windows policy
- Update filter-repo-rs/tests/integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #16 from Cactusinhand/codex/add-tests-and-documentation-updates

Expand integration coverage for path/ref maps and docs
- Update help print message
- Add user case in Readme
- Improve parent handling for pruned commits
- Merge pull request #17 from Cactusinhand/codex/track-preserved-parent-tags-in-commits

Handle parent dedup for pruned commits
- Add short hash rewriting for commit and tag messages
- Update short hash mapper on new commits
- Refine short hash rewriting to avoid extra allocations
- Merge pull request #18 from Cactusinhand/codex/add-short-hash-detection-and-replacement-logic

Add short hash rewriting for commit and tag messages
- Add regex-based path filtering support
- Update filter-repo-rs/src/filechange.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #19 from Cactusinhand/codex/add-path_regexes-support-and-tests

Add regex-based path filtering support
- Tear up large file integration.rs into module
- Update filter-repo-rs/tests/errors.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/platform.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Improve unicode path heavy load memory test
- Merge pull request #20 from Cactusinhand/refact/test-refact

refact: Breaks down large files integration.rs into modularity
- Update readme
- Update docs
- Update docs
- Gate analysis threshold flags behind debug
- Update filter-repo-rs/src/opts.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #21 from Cactusinhand/codex/adjust-analysis-micro-tuning-flags

Gate analysis tuning flags behind debug mode
- Gate fast-export passthrough flags behind debug
- Update filter-repo-rs/tests/cli.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Parameterize fast export gating tests
- Merge pull request #22 from Cactusinhand/codex/adjust-implementation-for-fast-export-passthrough

Gate fast-export passthrough flags behind debug mode
- Gate debug-only flags and help
- Guard --cleanup aggressive behind debug mode
- Merge pull request #23 from Cactusinhand/codex/implement-debug-options-and-help-updates

Gate debug-only options behind debug mode
- Add boolean cleanup flag and deprecate modes
- Update filter-repo-rs/src/finalize.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/src/opts.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Add legacy aggressive cleanup CLI test
- Warn on git gc failure
- Merge pull request #24 from Cactusinhand/codex/update-cleanup-mode-handling-in-opts.rs

Make --cleanup a boolean flag and deprecate legacy modes
- Add TOML config loading
- Guard config threshold overrides behind debug mode
- Refactor analyze threshold application
- Merge pull request #25 from Cactusinhand/codex/add-configuration-loading-and-tests

filter-repo-rs: add TOML config loading
- Warn on legacy analysis and cleanup flags
- Clarify legacy warning helper
- Merge pull request #26 from Cactusinhand/codex/deprecate-old-flags-and-update-documentation

Warn on legacy analysis threshold CLI usage
- Merge pull request #27 from Cactusinhand/codex/add-example-config-and-update-documentation

docs: share example config and refresh cli guidance
- Add integration coverage for cleanup, config overrides, and safe defaults
- Update filter-repo-rs/tests/cleanup.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/common/mod.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Improve git spy repo command detection
- Improve git spy parsing and cleanup expectations
- Merge pull request #28 from Cactusinhand/codex/add-integration-tests-and-documentation-updates

Add integration coverage for cleanup, config overrides, and safe defaults
- Fix windows-specific warnings in test helpers
- Merge pull request #29 from Cactusinhand/codex/fix-unreachable-expression-and-unused-variable

Fix windows-specific warnings in test helpers
- Update help print message
- Clarify untracked file check and skip for bare repos
- Update filter-repo-rs/src/sanity.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Merge pull request #30 from Cactusinhand/codex/handle-uncommitted-changes-in-stash

Gate untracked sanity check to non-bare repositories
- Cargo fmt
- Avoid deleting refs when rename only changes case
- Update
- Merge pull request #31 from Cactusinhand/codex/fix-tag-rename-to-preserve-tags

Prevent deleting case-only renamed refs
- Show full object hashes in analyze report
- Refine analyze tables to avoid cloning
- Merge pull request #32 from Cactusinhand/codex/fix-short-hash-value-for-big-blob

Show full object hashes in analyze report
- Allow underscores in max-blob-size argument
- Update filter-repo-rs/src/opts.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #33 from Cactusinhand/codex/fix-terminal-support-for-large-numbers

Allow underscores in max-blob-size argument
- Make sanity check more robust
- Update filter-repo-rs/src/sanity.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Handle git config errors with stderr context
- Refactor remaining sanity checks into helpers
- Refine preflight error propagation
- Skip unpushed check for repositories without tracking branches
- Ensure git config helpers read highest precedence value
- Make git config tests handle platform defaults
- Refine git helpers usage across pipeline
- Merge pull request #34 from Cactusinhand/feat/sanity

Make sanity check more robust
- Update filter-repo-rs/tests/already_ran_integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/errors.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Merge pull request #35 from Cactusinhand/refact/sanity-check

refactor sanity check
- Introduce unified error handling
- Preserve io::Error sources in SanityCheckError
- Merge pull request #36 from Cactusinhand/codex/introduce-filterrepoerror-for-error-handling

Introduce unified error handling
- Update
- Merge pull request #37 from Cactusinhand/update-test

update
- Probe git capabilities and gate features
- Reuse stored capabilities
- Merge pull request #38 from Cactusinhand/codex/add-git-capability-probe-and-tests

filter-repo: probe git capabilities and gate features
- Add size suffix parsing for --max-blob-size
- Refine max blob size multiplier constants
- Refactor max blob size CLI tests
- Merge pull request #39 from Cactusinhand/codex/add-format-validation-for-max-blob-size

Add size suffix parsing for --max-blob-size
- Merge pull request #40 from Cactusinhand/codex/verify-correctness-of-statements

docs: align convergence status with implemented safety checks
- Adjust analysis output
- Update filter-repo-rs/src/analysis.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Handle fast-export stdout absence and avoid rev-list hang
- Dedupe blob path assignment loop
- Refactor find_blob_context chaining
- Merge pull request #41 from Cactusinhand/feat/analyze

adjust analysis output
- Fix rename sanitize helper
- Merge pull request #42 from Cactusinhand/perf/optimize

perf:update
- Update
- Update filter-repo-rs/src/stream.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/src/filechange.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update
- Update
- Merge pull request #43 from Cactusinhand/perf/optimiz-2

update
- Update some tests
- Update filter-repo-rs/tests/paths_refs_integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Update filter-repo-rs/tests/paths_refs_integration.rs

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #44 from Cactusinhand/update-test

update some tests
- Update merge strategy
- Update README.zh-CN.md

Co-authored-by: gemini-code-assist[bot] <176961590+gemini-code-assist[bot]@users.noreply.github.com>
- Merge pull request #45 from Cactusinhand/merge-strategy

update merge strategy
- Update
- Handle windows path case
- Add --force to CLI path normalization tests
- Merge pull request #46 from Cactusinhand/feat/window-path-enhance

handle windows path case
- Support in --replace-text files for blob content replacements
- Implemented pruning/parent strategy switches
- Add more tests

Fix octopus merge helper and empty merge test

Fix special character replacement expectations
- Tweat testcase to more robust
- Merge pull request #47 from Cactusinhand/fix-test-failure

Fix test failure
- Update test
- Adjust --cleanup implement
- Add LICENSE
- Address some cargo clippy issue
- Concern some cargo-clippy issue

### 🚜 Refactor

- Deduplicate regex rule parsing
- Reuse analyze threshold overrides
- Refactor sanity check
- Deduplicate CLI path normalization

### 📚 Documentation

- Share example config and refresh cli guidance
- Sync cli convergence and safety status
- Docs update
- Update

### ⚡ Performance

- Update

### 🧪 Testing

- Assert git setup succeeds
- Restore missing coverage and validate options
- Import filtered stream when checking merge parents

### ⚙️ Miscellaneous Tasks

- Adjust analyze logic
<!-- Releases -->

[1.0.0-beta]: https://github.com/Cactusinhand/filter-repo-rs/releases/tag/v1.0.0-beta
[0.1.0]: https://github.com/Cactusinhand/filter-repo-rs/releases/tag/v0.1.0
