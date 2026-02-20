# filter-repo-rs: Current Status, Limitations, and MVP Plan

## Summary (2025-09-20)

A minimal Rust prototype of git-filter-repo is working end-to-end on real repositories. It builds a streaming fast-export -> filter -> fast-import pipeline, keeps debug streams, and implements several core features with Windows compatibility fixes. This document tracks what's done, known limitations, and the remaining MVP scope.

## Features Implemented

- Pipeline & Debug
  - Streams `git fast-export` -> filters -> `git fast-import`.
  - Debug copies: always writes `.git/filter-repo/fast-export.filtered`; writes `.git/filter-repo/fast-export.original` only in debug/report contexts (e.g., `--debug-mode`, `--write-report`, or when size-based sampling is enabled) to reduce I/O on very large repos.
  - Fast-export flags: `--show-original-ids --signed-tags=strip --tag-of-filtered-object=rewrite --fake-missing-tagger --reference-excluded-parents --use-done-feature`.
  - Also enabled: `-c core.quotepath=false`, `--reencode=yes`, `--mark-tags`.
  - Debug gating: `--debug-mode` / `FRRS_DEBUG` exposes fast-export passthrough knobs and analysis thresholds; baseline `--help` hides them.
  - Fast-import runs with `-c core.ignorecase=false` and exports marks to `.git/filter-repo/target-marks`.

- Configuration & docs/test parity
  - `.filter-repo-rs.toml` loads from the source repo (or `--config` override); debug-only thresholds stay gated.
  - Shared example lives at `docs/examples/filter-repo-rs.toml`, and integration tests reference it to prevent drift.

- Refactor & Module Layout
  - `main.rs`: minimal; delegates to `stream::run()`.
  - `stream.rs`: orchestrates the streaming loop (reads from fast-export, routes to modules, writes to fast-import and debug files).
  - `finalize.rs`: end-of-run flush of buffered lightweight tags, process waits, write `ref-map`/`commit-map`, optional `git reset --hard`.
  - `commit.rs`: commit header rename (tags/branches), per-line commit processing, message data handling, keep/prune decision, alias builder.
  - `tag.rs`: annotated tag block processing/dedupe plus lightweight tag reset helpers (reset header + capture next `from`).
  - `filechange.rs`: M/D/deleteall path filtering, prefix renames, C-style dequote/enquote, Windows path sanitization.
  - `pipes.rs`, `gitutil.rs`, `opts.rs`, `pathutil.rs`, `message.rs`: process setup, plumbing, CLI, utilities.

- Message Editing
  - `--replace-message FILE`: literal and regex (via `regex:` lines) byte-based replacements applied to commit and tag messages.

- Author/Committer Identity Rewriting
  - `--mailmap FILE`: rewrites author/committer identity lines using mailmap-style mappings.
  - `--author-rewrite FILE`: rewrites author identity text with `old==>new` rules.
  - `--committer-rewrite FILE`: rewrites committer identity text with `old==>new` rules.
  - `--email-rewrite FILE`: rewrites only email portions inside identity lines with `old==>new` rules.
  - Precedence: when `--mailmap` is provided, mailmap rewriting is applied and the explicit
    author/committer/email rewrite files are ignored for identity lines.

- Blob Filtering
  - `--replace-text FILE`: literal byte-based replacements applied to blob contents.
  - Regex and glob replacements in `--replace-text` are supported via `regex:` and `glob:` rules
    in the replacement file.
  - `glob:` patterns support `*` (match any characters) and `?` (match single character) wildcards.
  - `--max-blob-size BYTES`: drops oversized blobs and deletes paths that reference them.
  - `--strip-blobs-with-ids FILE`: drops blobs by 40-hex id (one per line).
  - Optional report (`--write-report`) writes a summary to `.git/filter-repo/report.txt`.
  - Optional post-import cleanup via boolean `--cleanup` (standard) and debug-only `--cleanup-aggressive`; legacy `--cleanup=<mode>` remains for now with migration warnings.

- Path Filtering & Renaming
  - `--path PREFIX`: include-only filtering of filechange entries (M/D/deleteall).
  - `--path-glob GLOB`: include via glob patterns (`*`, `?`, `**`).
  - `--path-regex REGEX`: include via Rust regex (bytes mode, repeatable).
  - `--invert-paths`: invert path selection (drop matches; keep others).
  - `--path-rename OLD:NEW` with helpers:
    - `--subdirectory-filter DIR` (equivalent to `--path DIR/ --path-rename DIR/:`).
    - `--to-subdirectory-filter DIR` (equivalent to `--path-rename :DIR/`).
- Windows path sanitization when rebuilding filechange lines.
- CLI enforces Git-style path rules across platforms:
  - converts '\' to '/' in `--path`, `--path-glob`, subdirectory filters, and path renames
  - rejects absolute paths (leading '/', '//' or Windows drive/UNC), and '.'/'..' segments

- Empty Commit Pruning & Merge Preservation
  - Prunes empty non-merge commits via fast-import `alias` of marks (old mark -> first parent), so downstream refs resolve.
  - Degenerate merges (merges that become <2 parents after filtering) can be pruned when empty via `--prune-degenerate {always|auto|never}` (default: auto). Use `--no-ff` to keep such merges.

- Tag Handling (Annotated-first)
  - Annotated tags: buffer entire tag blocks, optionally rename via `--tag-rename OLD:NEW`, dedupe by final ref, emit once.
  - Lightweight tags: buffer `reset refs/tags/<name>` + following `from` line, flush before `done`, skip if overshadowed by annotated tag.
  - Commit headers targeting `refs/tags/*` are renamed under `--tag-rename OLD:NEW`.
  - Ref cleanup: `.git/filter-repo/ref-map` written; old tag refs deleted only if the new ref exists.

- Branch Handling
  - `--branch-rename OLD:NEW`: applied to commit headers and `reset refs/heads/*`.
  - Safe deletion of old branches only when the new exists; recorded in `ref-map`.
  - HEAD: if original target is missing, update to the mapped target under `--branch-rename`, else to the first updated branch.

- Commit/Ref Maps
  - `.git/filter-repo/commit-map`: old commit id (`original-oid`) -> new commit id (via exported marks; falls back by scanning filtered stream).
  - `.git/filter-repo/ref-map`: old ref -> new ref for tag/branch renames.

- Safety & Sanity Enforcement
  - `--enforce-sanity` blocks risky situations including case-insensitive and Unicode-normalization reference collisions, stash presence, oversized reflogs, replace-ref-only loose objects, dirty or untracked worktrees, multiple worktrees, and stale remote setups unless `--force` is set.

## Known Limitations (to be addressed)

- Path parsing/quoting
  - We rebuild quoted paths with minimal C-style unescape/escape only when the original was quoted; pure pass-through M/D lines rely on fast-export quoting.

- Filtering semantics
  - Include-by-prefix (`--path`), glob (`--path-glob`), regex (`--path-regex`), and invert (`--invert-paths`) supported.
  - Regex path matching uses the Rust `regex` crate (bytes). Look-around and backreferences are unsupported, and complex patterns may have higher CPU cost; anchor expressions when possible.
  - Blob replacements support `regex:` (full regex syntax) and `glob:` (simple `*` and `?` wildcards) syntaxes in replacement files.


- Merge/degen handling
  - We preserve merges but do not trim redundant parents or implement `--no-ff` semantics.
  - Commit-map currently records kept commits; pruned commits can be recorded as `old -> None` in a future enhancement.

- Incremental/state-branch
  - No `--state-branch` support (marks import/export to a branch and incremental reruns). Marks are exported to a file only.

- LFS & large repos
  - No LFS detection/orphan reporting, no size-based skipping.

- Encoding & hash rewriting
  - Messages are re-encoded to UTF-8 (`--reencode=yes`).
  - Commit/tag message short/long hash translation is implemented using `commit-map` (old → new);
    a `--preserve-commit-hashes` flag to disable this behavior is not yet available.

 - Path compatibility policy
 - `--path-compat-policy={sanitize|skip|error}` is available (default `sanitize`).
  - Policy is currently enforced only when running on Windows hosts.
  - When policy matches paths, a dedicated `.git/filter-repo/windows-path-report.txt` audit file is emitted.
  - `report.txt` / `report.json` include a Windows path compatibility summary when enabled via report flags.

## Non-goals

- Callback framework (filename/refname/blob/commit/tag/reset/message/name/email): not planned for this project. We prefer explicit CLI flags and focused features over embedding a general callback API layer.

## Scope & Priorities (overview)

- Value‑focused features: see docs/SCOPE.md (English) and docs/SCOPE.zh-CN.md (Chinese) for high‑value items, pain points → solutions, and “why raw Git is hard”.
- Boundaries: core vs. non‑goals vs. “re‑evaluate later” are tracked in the SCOPE docs; check alignment before adding new flags.

## CLI Convergence

- See docs/CLI-CONVERGENCE.zh-CN.md for the proposed CLI consolidation plan (core vs. hidden/debug, merged semantics, config file for analysis thresholds, and deprecation strategy).
- Analysis threshold "micro-tuning" flags (`--analyze-*-warn`) are now hidden by default and require `--debug-mode` or `FRRS_DEBUG=1`; core help surfaces only `--analyze`, `--analyze-json`, and `--analyze-top`. As of the latest prototype, these legacy flags emit a one-time warning pointing to `analyze.thresholds.*` config keys, and the help text references them only as compatibility shims.
- Legacy `--cleanup=<mode>` syntax continues to parse for now, but prints a one-time warning steering users to the boolean `--cleanup` or debug-only `--cleanup-aggressive`. Stage-3 toggles exist in the parser to disable legacy cleanup/analyze syntax once compatibility can be removed, and can now be exercised at runtime with `FRRS_STAGE3_DISABLE_LEGACY_CLEANUP=1` / `FRRS_STAGE3_DISABLE_LEGACY_ANALYZE_FLAGS=1`.

### Progress checklist

- [x] Baseline `--help` hides debug-only flags while `--debug-mode` / `FRRS_DEBUG` surfaces fast-export passthrough, cleanup, and analysis overrides.
- [x] Debug gating is enforced for cleanup semantics (`--no-reset`, `--cleanup-aggressive`) and fast-export knobs; errors guide users to enable debug mode when necessary.
- [x] `.filter-repo-rs.toml` config loading is wired in with a shared example at `docs/examples/filter-repo-rs.toml`; integration tests consume the same sample to keep docs and behavior aligned.
- [x] Stage-3 toggles remain available to drop legacy cleanup/analysis flags entirely once ecosystem consumers finish migrating (runtime exercise via `FRRS_STAGE3_DISABLE_LEGACY_CLEANUP` and `FRRS_STAGE3_DISABLE_LEGACY_ANALYZE_FLAGS`).

## MVP Scope (target) and Gap

MVP Goal: a stable, performant subset that covers the most common workflows:

- End-to-end pipeline with debug streams.
- Message editing (`--replace-message`).
- Path include + basic rename/subdirectory helpers (`--path`, `--path-rename`, helpers), glob, invert.
- Empty-commit pruning with merge preservation.
- Tag/Branch renaming (annotated-first for tags) including resets; safe old-ref deletion.
- Commit/Ref maps.
- Windows compatibility (quotepath, ignorecase, path sanitization).

Remaining for MVP polish:

1) Path semantics
   - Improved parser robustness for filechange lines with CRLF endings: quoted/unquoted `M/D/C/R` and `deleteall` now parse instead of falling back to pass-through, so path filtering/renaming and requoting still apply.

2) Refs finalization
   - Done: batch atomic updates for branches/tags via `git update-ref --stdin`; HEAD updated via `git symbolic-ref`.

3) Commit-map completeness
   - Done: pruned commits recorded as `old -> 0000000000000000000000000000000000000000`; commit-map always written.

4) Path compatibility policy
   - Done: `--path-compat-policy=[sanitize|skip|error]` (default sanitize), plus per-path compatibility reporting.

5) Tests & docs
   - Integration coverage now includes path filtering + branch rename + HEAD update, commit-map pruned entries, annotated vs lightweight tag scenarios, path-rename matrices, and encoding/quoting notes (including escaped quote/backslash + octal UTF-8 path round-trips).

## Recent Test Coverage Additions

- Combined rename + message rewrite
  - Annotated tag rename with tag message rewrite (literal replacements)
  - Branch rename with annotated tag message rewrite
- HEAD finalization
  - `HEAD` moves to the renamed branch via `git symbolic-ref`, with fallbacks when needed
- Short‑hash remap across runs
  - Messages referencing short commit IDs are remapped on a subsequent run using `commit-map`
- Binary integrity
  - When text replacement rules do not match a blob payload, the blob OID remains unchanged
