Test Suite Overview
===================

This crate’s tests were split from a single large integration file into focused, smaller suites. Each file under `tests/` targets a feature area and uses shared helpers from `tests/common`.

Layout
- `common/` — shared helpers (mktemp, init_repo, run_git, run_tool, etc.)
- `analyze.rs` — analyze mode and JSON report
- `backup.rs` — backup bundle behavior (see Windows note)
- `blobs.rs` — core max-blob-size checks
- `blobs_more.rs` — extended blob-size scenarios and edge cases
- `errors.rs` — invalid inputs and graceful error handling
- `maps.rs` — commit-map and ref-map content
- `memory.rs` — memory-related stress and repeated operations
- `messages.rs` — commit/tag message rewriting and short-hash remapping
- `merge.rs` — merge/parent deduplication when branches are pruned
- `multi_feature.rs` — combined flags (paths/renames/size/invert) interactions
- `paths.rs` — path selection, globs, regex, quoting behavior
- `performance.rs` — larger data set timings and scaling smoke checks
- `platform.rs` — cross‑platform path, Unicode, line endings, permissions
- `rename.rs` — branch and tag renames (HEAD tracking)
- `replace.rs` — replace-text content filters
- `reports.rs` — human report counters and samples
- `sensitive.rs` — sensitive/partial modes vs remotes
- `stream.rs` — custom fast-export stream overrides
- `unit.rs` — small focused unit-style checks

Running
- Run everything: `cargo test -p filter-repo-rs`
- Run a subset: `cargo test -p filter-repo-rs --test blobs_more`

Git version
- Some tests rely on newer fast-import/export flags (e.g., `--date-format=raw-permissive`). If you see errors like “unknown --date-format argument raw-permissive”, please upgrade Git to a recent version.

Windows notes
- The backup tests may fail under some MSYS/Cygwin Git-for-Windows setups with path mixing like `/cygdrive/.../D:\...` when creating bundles. This is environment-specific path translation rather than a logic error.
  - Workarounds:
    - Use a recent native Git for Windows without MSYS path translation for tests.
    - Or run on WSL/Linux/macOS for these backup tests.
    - Or adjust `backup.rs` locally to use relative paths on Windows.

Conventions
- Prefer small, focused tests with clear setup and assertions.
- Use `run_git(...)` for shelling out and `init_repo()` for isolated repos under `target/it/`.
- When asserting on fast-export output, normalize quoting via `dequote_c_style_bytes` if needed.

Adding new tests
- Pick or create a `*.rs` file that matches the feature area.
- Reuse helpers from `tests/common` and follow the existing patterns to keep tests readable and deterministic.

