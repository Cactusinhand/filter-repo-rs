# filter-repo-rs: Current Status, Limitations, and MVP Plan

## Summary (2025-09-14)

A minimal Rust prototype of git-filter-repo is working end-to-end on real repositories. It builds a streaming fast-export -> filter -> fast-import pipeline, keeps debug streams, and implements several core features with Windows compatibility fixes. This document tracks what's done, known limitations, and the remaining MVP scope.

## Features Implemented

- Pipeline & Debug
  - Streams `git fast-export` -> filters -> `git fast-import`.
  - Saves debug copies at `.git/filter-repo/fast-export.{original,filtered}`.
  - Fast-export flags: `--show-original-ids --signed-tags=strip --tag-of-filtered-object=rewrite --fake-missing-tagger --reference-excluded-parents --use-done-feature`.
  - Also enabled: `-c core.quotepath=false`, `--reencode=yes`, `--mark-tags`.
  - Fast-import runs with `-c core.ignorecase=false` and exports marks to `.git/filter-repo/target-marks`.

- Refactor & Module Layout
  - `main.rs`: minimal; delegates to `stream::run()`.
  - `stream.rs`: orchestrates the streaming loop (reads from fast-export, routes to modules, writes to fast-import and debug files).
  - `finalize.rs`: end-of-run flush of buffered lightweight tags, process waits, write `ref-map`/`commit-map`, optional `git reset --hard`.
  - `commit.rs`: commit header rename (tags/branches), per-line commit processing, message data handling, keep/prune decision, alias builder.
  - `tag.rs`: annotated tag block processing/dedupe plus lightweight tag reset helpers (reset header + capture next `from`).
  - `filechange.rs`: M/D/deleteall path filtering, prefix renames, C-style dequote/enquote, Windows path sanitization.
  - `pipes.rs`, `gitutil.rs`, `opts.rs`, `pathutil.rs`, `message.rs`: process setup, plumbing, CLI, utilities.

- Message Editing
  - `--replace-message FILE`: literal byte-based replacements applied to commit and tag messages.

- Blob Filtering
  - `--replace-text FILE`: literal byte-based replacements applied to blob contents.
  - Regex replacements in `--replace-text` supported via `regex:` rules in the replacement file.
  - `--max-blob-size BYTES`: drops oversized blobs and deletes paths that reference them.
  - `--strip-blobs-with-ids FILE`: drops blobs by 40-hex id (one per line).
  - Optional report (`--write-report`) writes a summary to `.git/filter-repo/report.txt`.
  - Optional post-import cleanup via `--cleanup [none|standard|aggressive]`.

- Path Filtering & Renaming
  - `--path PREFIX`: include-only filtering of filechange entries (M/D/deleteall).
  - `--path-glob GLOB`: include via glob patterns (`*`, `?`, `**`).
  - `--path-regex REGEX`: include via Rust regex (bytes mode, repeatable).
  - `--invert-paths`: invert path selection (drop matches; keep others).
  - `--path-rename OLD:NEW` with helpers:
    - `--subdirectory-filter DIR` (equivalent to `--path DIR/ --path-rename DIR/:`).
    - `--to-subdirectory-filter DIR` (equivalent to `--path-rename :DIR/`).
  - Windows path sanitization when rebuilding filechange lines.

- Empty Commit Pruning & Merge Preservation
  - Prunes empty non-merge commits via fast-import `alias` of marks (old mark -> first parent), so downstream refs resolve.
  - Preserves merge commits (2+ parents) even if filtered to zero filechanges.

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

## Known Limitations (to be addressed)

- Path parsing/quoting
  - We rebuild quoted paths with minimal C-style unescape/escape only when the original was quoted; pure pass-through M/D lines rely on fast-export quoting.

- Filtering semantics
  - Include-by-prefix (`--path`), glob (`--path-glob`), regex (`--path-regex`), and invert (`--invert-paths`) supported.
  - Regex path matching uses the Rust `regex` crate (bytes). Look-around and backreferences are unsupported, and complex patterns may have higher CPU cost; anchor expressions when possible. Regex blob replacements are supported by default through the `regex:` syntax in replacement files.


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

 - Windows path policy
 - Sanitization is always on for rebuilt lines; no user-selectable policy (sanitize|skip|error) yet.
  - Integration tests cover the sanitization behavior (mirrors existing always-sanitize policy) alongside
    path filtering/renames, commit/ref map emission, and ref finalization. Run them with
    `cargo test -p filter-repo-rs` (requires Git in `PATH`).

## Non-goals

- Callback framework (filename/refname/blob/commit/tag/reset/message/name/email): not planned for this project. We prefer explicit CLI flags and focused features over embedding a general callback API layer.

## Scope & Priorities (overview)

- Value‑focused features: see docs/SCOPE.md (English) and docs/SCOPE.zh-CN.md (Chinese) for high‑value items, pain points → solutions, and “why raw Git is hard”.
- Boundaries: core vs. non‑goals vs. “re‑evaluate later” are tracked in the SCOPE docs; check alignment before adding new flags.

## CLI Convergence

- See docs/CLI-CONVERGENCE.zh-CN.md for the proposed CLI consolidation plan (core vs. hidden/debug, merged semantics, config file for analysis thresholds, and deprecation strategy).

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
   - Robust dequote/enquote for pass-through lines when needed.

2) Refs finalization
   - Consider batch updates for branches/HEAD via `git update-ref --stdin` to mirror git-filter-repo’s behavior.

3) Commit-map completeness
   - Optionally record pruned commits as `old -> None` to make the map exhaustive.

4) Windows path policy flag
   - `--windows-path-policy=[sanitize|skip|error]` (default sanitize) with a per-path report for changed names.

5) Tests & docs
   - Broaden tests for path/refs combinations; document limitations and interop notes (encoding, quoting).
