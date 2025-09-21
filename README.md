filter-repo-rs (Rust prototype of git-filter-repo)
==================================================

filter-repo-rs is a Rust prototype reimplementation of [git-filter-repo](https://github.com/newren/git-filter-repo).

It streams `git fast-export` -> in‑process filters -> `git fast-import`, writes
debug streams, and focuses on safe, fast, cross‑platform operation (including Windows).

- [English](README.md) | [中文](README.zh-CN.md)

Status: prototype. Not feature‑complete with Python, but usable for common workflows.

To quickly understand this tool, please see Use Cases:

Use Cases
---------

1) Remove leaked secrets across history (files and optionally messages)

- Goal: scrub sensitive strings from all commits across all refs.
- Suggested steps:
  1. Backup first (strongly recommended):
     ```sh
     filter-repo-rs --backup --refs --all
     ```
  2. Author replacement rules (literal + regex both supported for file contents):
     ```sh
     # redact.txt
     SECRET_TOKEN==>REDACTED
     regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED
     ```
  3. Clean sensitive data across refs (use `--sensitive` to include remote refs if present):
     ```sh
     filter-repo-rs \
       --sensitive \
       --replace-text redact.txt \
       --write-report
     ```
  4. If commit/tag messages contain sensitive data, add message rules as well (currently literal rules only):
     ```sh
     filter-repo-rs --replace-message msg_rules.txt
     ```
  5. Force‑push new history:
     ```sh
     git push --force --all
     git push --force --tags
     ```
  6. Coordinate with team/CI to prevent old history from re‑appearing (clear caches, forks, etc.).

2) Scrub sensitive commit/tag messages

- Prepare message rules (literal for now):
  ```sh
  # messages.txt
  password==>[removed]
  ```
- Run:
  ```sh
  filter-repo-rs --replace-message messages.txt --write-report
  ```
- Combine with `--backup`, `--sensitive`, and `--dry-run` for safe rehearsal/full coverage.

3) Reduce repository size by removing large binaries

- Inspect first:
  ```sh
  filter-repo-rs --analyze
  filter-repo-rs --analyze --analyze-json
  ```
- Remove by threshold (and delete referencing paths):
  ```sh
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```
- Or remove by explicit blob IDs:
  ```sh
  filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
  ```
- Consider moving large media to Git LFS or external storage to avoid future bloat.

4) Bulk renaming of tags/branches

- Rename tag prefixes:
  ```sh
  filter-repo-rs --tag-rename v1.:legacy/v1.
  ```
- Rename branch prefixes:
  ```sh
  filter-repo-rs --branch-rename feature/:exp/
  ```

5) Adjust directory layout

- Extract a subdirectory as the new root (e.g., splitting a monorepo component):
  ```sh
  filter-repo-rs --subdirectory-filter frontend
  ```
- Move the current root under a subdirectory:
  ```sh
  filter-repo-rs --to-subdirectory-filter app/
  ```
- Bulk rename a path prefix:
  ```sh
  filter-repo-rs --path-rename old/:new/
  ```

6) Safety tips and common switches

- Dry‑run without updating refs: `--dry-run`
- Write an audit summary: `--write-report`
- Backup before rewriting: `--backup [--backup-path PATH]`
- Sensitive mode (cover all remote refs): `--sensitive` (with `--no-fetch` to skip fetching)
- Partial rewrite (keep existing remotes/refs): `--partial`
- Bypass protections if required: `--force` (use with care)

7) CI health checks

- In CI, run:
  ```sh
  filter-repo-rs --analyze --analyze-json \
    --analyze-large-blob 10_000_000 \
    --analyze-commit-msg-warn 4096 \
    --analyze-max-parents-warn 8
  ```
- Use emitted warnings to block oversize commits and monitor repo growth trends.

Quick start
-----------

Run inside a Git repository (or pass `--source`/`--target`):

```sh
filter-repo-rs \
  --source . \
  --target . \
  --refs --all \
  --date-order \
  --replace-message replacements.txt
```

Features
--------

- Streaming pipeline
  - `fast-export` -> filters -> `fast-import`, with debug copies saved under `.git/filter-repo/`.
  - Core fast-export flags enabled: `--show-original-ids`, `--signed-tags=strip`,
    `--tag-of-filtered-object=rewrite`, `--fake-missing-tagger`,
    `--reference-excluded-parents`, `--use-done-feature`.
  - `fast-import` runs with `-c core.ignorecase=false` and exports marks to `.git/filter-repo/target-marks`.

- Path selection & rewriting
  - Include by prefix `--path`, glob `--path-glob` (`*`, `?`, `**`), or regex `--path-regex` (Rust regex; no look‑around/backrefs).
  - `--invert-paths` to invert selection; `--path-rename OLD:NEW` for prefix renames.
  - Helpers: `--subdirectory-filter DIR` and `--to-subdirectory-filter DIR`.

- Blob filtering & redaction
  - `--replace-text FILE` for content replacements; supports literal rules and `regex:` rules
    in the same file (e.g., `regex:api_key-[0-9]+==>REDACTED`).
  - `--max-blob-size BYTES` drops large blobs and removes paths that reference them.
  - `--strip-blobs-with-ids FILE` drops listed 40‑hex blob IDs.

- Commit, tag, and refs
  - `--replace-message FILE` applies literal replacements in commit/tag messages.
  - Short/long commit hashes in messages are rewritten to new IDs using the generated `commit-map`.
  - `--tag-rename` and `--branch-rename` rename by prefix; annotated tags are deduped and emitted once.
  - Empty non‑merge commits are pruned via `alias` to the first parent mark; merges are preserved.
  - Safe ref updates and HEAD selection after import.

- Safety, backup, and analysis
  - Optional preflight checks; `--backup` creates a bundle before rewriting; `--write-report` summarizes actions.
  - Analyze mode: `--analyze` (human) or `--analyze --analyze-json` (machine) to inspect repository health.

Requirements
------------

- Git available on PATH (a recent version recommended)
- Rust toolchain (stable)
- Linux/macOS/Windows supported

Build
-----

```sh
cargo build -p filter-repo-rs --release
```

Testing
-------

```sh
cargo test -p filter-repo-rs
```

The suite sets up temporary repos under `target/it/`, requires Git on PATH,
and writes debug artifacts (commit-map, ref-map, report) in each ephemeral repo.

CLI overview: core vs debug layers
----------------------------------

Core CLI (always available; see [docs/SCOPE.md](docs/SCOPE.md) for prioritized scenarios and [docs/PARITY.md](docs/PARITY.md) for parity/safety context):

- Repository & refs
  - `--source DIR`, `--target DIR` (default `.`), `--refs` (repeatable, defaults to `--all`)
  - `--no-data` forwarded to fast-export

- Paths
  - `--path`, `--path-glob`, `--path-regex`, `--invert-paths`
  - `--path-rename OLD:NEW`, `--subdirectory-filter DIR`, `--to-subdirectory-filter DIR`

- Content & blobs
  - `--replace-text FILE`, `--max-blob-size BYTES`, `--strip-blobs-with-ids FILE`

- Messages & refs
  - `--replace-message FILE`, `--tag-rename OLD:NEW`, `--branch-rename OLD:NEW`

- Behavior & output
  - `--write-report`, `--cleanup [none|standard|aggressive]`, `--quiet`, `--no-reset`
  - `--backup [--backup-path PATH]`, `--dry-run`
  - `--partial`, `--sensitive [--no-fetch]`, `--force`, `--enforce-sanity`
  - Analysis entry points: `--analyze`, `--analyze-json`, `--analyze-top`. Configure thresholds via `.filter-repo-rs.toml` or `--config` (see [docs/examples/filter-repo-rs.toml](docs/examples/filter-repo-rs.toml)).

Debug overlays *(enable with `--debug-mode` or `FRRS_DEBUG=1`; legacy compatibility toggles stay hidden by default)*:

- Analysis thresholds / legacy overrides
  - `--analyze-total-warn`, `--analyze-total-critical`, `--analyze-large-blob`, `--analyze-ref-warn`, `--analyze-object-warn`, `--analyze-tree-entries`, `--analyze-path-length`, `--analyze-duplicate-paths`, `--analyze-commit-msg-warn`, `--analyze-max-parents-warn`
  - Each emits a warning pointing to the config keys in `.filter-repo-rs.toml`.

- Fast-export passthrough knobs
  - `--date-order`, `--no-reencode`, `--no-quotepath`, `--no-mark-tags`, `--mark-tags`

- Cleanup & stream overrides
  - `--no-reset`, `--cleanup-aggressive`, `--fe_stream_override`

Examples
--------

- Remove leaked secrets from history

  ```sh
  # 1) Backup (recommended)
  filter-repo-rs --backup --refs --all

  # 2) Write replacement rules for file contents
  cat > redact.txt <<EOF
  SECRET_TOKEN==>REDACTED
  regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED
  EOF

  # 3) Apply redaction and write a summary report
  filter-repo-rs --sensitive --replace-text redact.txt --write-report

  # 4) Force-push new history
  git push --force --all && git push --force --tags
  ```

- Clean up sensitive commit/tag messages (literal rules)

  ```sh
  cat > messages.txt <<EOF
  password==>[removed]
  EOF
  filter-repo-rs --replace-message messages.txt --write-report
  ```

- Shrink repository by removing large blobs

  ```sh
  # Inspect first
  filter-repo-rs --analyze
  filter-repo-rs --analyze --analyze-json

  # Drop blobs over 5MB and delete their paths
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```

- Restructure paths

  ```sh
  # Extract a subdirectory as the new root
  filter-repo-rs --subdirectory-filter frontend

  # Move the current root under a subdirectory
  filter-repo-rs --to-subdirectory-filter app/

  # Bulk rename a path prefix
  filter-repo-rs --path-rename old/:new/
  ```

Backup and restore
------------------

`--backup` creates a timestamped bundle under `.git/filter-repo/` by default.

Restore from bundle:

```sh
git clone /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle restored-repo
# or
git init restored-repo && cd restored-repo
git bundle unbundle /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle
git symbolic-ref HEAD refs/heads/<branch-from-bundle>
```

Behavior highlights
-------------------

- Debug streams: `.git/filter-repo/fast-export.{original,filtered}`.
- Empty commit pruning via `alias` for non-merge commits; merges are preserved.
- Tags
  - Annotated tags: buffered, optionally renamed, deduped, emitted once.
  - Lightweight tags: `reset`/`from` buffered and flushed before `done`.
- Refs
  - Old refs deleted only after the new ones exist; `ref-map` records renames.
  - HEAD is updated to a valid branch (mapped under `--branch-rename` when possible).
- Remotes
  - Full runs (not `--partial`) migrate `refs/remotes/origin/*` to `refs/heads/*` before filtering.
  - In non‑sensitive runs, the `origin` remote is removed after completion to avoid accidental pushes to old history.
  - In sensitive mode, all refs may be fetched (unless `--no-fetch`) and origin is kept.

Artifacts
---------

- `.git/filter-repo/commit-map`: original commit ID -> new commit ID
- `.git/filter-repo/ref-map`: original ref -> new ref
- `.git/filter-repo/report.txt`: counts and sample paths for stripped/modified blobs (when `--write-report`)
- `.git/filter-repo/target-marks`: marks map table
- `.git/filter-repo/fast-export.original`: git fast-export original output
- `.git/filter-repo/fast-export.filtered`: git fast-export filtered output
- `.git/filter-repo/1758125153-834782600.bundle`: backup file


Windows notes
-------------

- Rebuilt paths are sanitized for Windows (reserved characters are replaced, trailing dots/spaces are trimmed).
- Some backup tests can be sensitive to MSYS/Cygwin path translation; see tests/README for workarounds.

Limitations (prototype)
-----------------------

- Merge simplification not implemented; degenerate merges are not pruned yet.
- No `--state-branch` (marks are exported to a file only).
- Windows path policy is fixed to "sanitize"(no skip/error modes yet).
 - Callback API is not planned for this project. Mailmap-based identity rewriting remains a possible future enhancement.
- `--replace-message` supports literal rules; regex rules are planned.
- Short-hash rewriting is enabled; a `--preserve-commit-hashes` toggle is planned.
- Human‑readable size parsing (e.g., `5M`) is not yet supported.

Roadmap / TODO (parity with Python git-filter-repo)
--------------------------------------------------

- Path features: `--paths-from-file`, `--use-base-name`, `--path-rename-match`/regex renames
- Messages: `--replace-message` support for `regex:`; `--preserve-commit-hashes`
- Blob sizes: accept `5M`/`2G` and alias `--strip-blobs-bigger-than`
 - Identity: mailmap (`--mailmap`, `--use-mailmap`)
- Merges: prune degenerate merges while preserving required ancestry
- Replace-refs & incremental: `--replace-refs …`, `--state-branch`, stash (`refs/stash`) rewrite
- Analysis & reports: LFS-related reporting; richer artifacts
- Windows path policy: `--windows-path-policy=[sanitize|skip|error]` + reporting
- Non-goal: Callback framework (filename/refname/blob/commit/tag/reset/message/name/email) — we do not plan to implement a callback API; prefer explicit CLI options.
- Safety defaults: consider stricter preflight by default; refine partial/sensitive guidance

More context:
- See [docs/PARITY.md](docs/PARITY.md) for Python parity and safety notes.
- See [docs/SCOPE.md](docs/SCOPE.md) for scope, priorities, and trade‑offs.
