filter-repo-rs (Rust Prototype)
===============================

This is a Rust reimplementation prototype of git-filter-repo. It streams
`git fast-export` -> filters -> `git fast-import`, keeps debug streams, and provides
several core features with Windows compatibility.

Build
-----

cd rust
cargo build -p filter-repo-rs --release

Run
---

Run from inside a Git repository (or pass `--source`/`--target`):

rust/target/release/filter-repo-rs \
  --source . \
  --target . \
  --refs --all \
  --date-order \
  --replace-message replacements.txt

Key Flags (prototype)
---------------------

- `--source DIR`, `--target DIR`: working directories (default `.`)
- `--ref|--refs REF`: repeatable; defaults to `--all`
- `--date-order`, `--no-data`: pass-through to fast-export
- `--quiet`, `--no-reset`: reduce noise / skip post-import reset
- `--replace-message FILE`: literal replacements for commit/tag messages.
  Each non-empty, non-comment line is `from==>to` or `from` (implies `***REMOVED***`).
- `--replace-text FILE`: literal replacements applied to blob contents (files). Same syntax
  as `--replace-message`. Lines starting with `regex:` are treated as regex rules
  (e.g., `regex:foo[0-9]+==>X`).
- `--path PREFIX`: include-only by prefix (repeatable; ORed)
- `--path-glob GLOB`: include by glob (supports `*`, `?`, `**`; repeatable; ORed)
- `--invert-paths`: invert selection (drop matches; keep others)
- `--path-rename OLD:NEW`: rename path prefix in file changes
- `--subdirectory-filter DIR`: equivalent to `--path DIR/ --path-rename DIR/:`
- `--to-subdirectory-filter DIR`: equivalent to `--path-rename :DIR/`
- `--tag-rename OLD:NEW`: rename tags starting with OLD to start with NEW
- `--branch-rename OLD:NEW`: rename branches starting with OLD to start with NEW
 - `--max-blob-size BYTES`: drop blobs larger than BYTES and delete paths that reference them.
 - `--strip-blobs-with-ids FILE`: drop blobs whose 40-hex ids (one per line) are listed.
 - `--cleanup [none|standard|aggressive]`: post-import cleanup (reflog expire + gc). Default `none`.
 - `--write-report`: write a summary to `.git/filter-repo/report.txt`.
  - `--partial`: partial rewrite; disables origin migration, ref cleanup, reflog gc.
  - `--sensitive` (aka sensitive-data removal): enables fetch-all refs to ensure coverage; implies skipping origin removal.
  - `--no-fetch`: do not fetch refs even in `--sensitive` mode.

Regex-based blob replacements are included in the default build.

Behavior Highlights
-------------------

- Saves debug streams to `.git/filter-repo/fast-export.{original,filtered}`.
- Empty-commit pruning (non-merges) via fast-import `alias` from old mark to first parent mark.
- Annotated tags: buffered, optionally renamed, deduped, and emitted once.
- Lightweight tags: `reset ...` + `from ...` pairs buffered and flushed before `done`.
- Safe deletion policy: old refs (tags/branches) deleted only after verifying the new exists.
- HEAD: if original HEAD target is missing, set HEAD to the mapped target under `--branch-rename`,
  otherwise to the first updated branch.
 - Origin migration and remote removal:
   - For full runs (not `--partial`): migrates `refs/remotes/origin/*` to `refs/heads/*` pre-run.
   - In non‑sensitive runs, removes the `origin` remote after completion (to prevent accidental pushes to the old history).
   - In sensitive mode (`--sensitive`), the tool attempts to fetch all refs (unless `--no-fetch`) to ensure complete coverage; origin is not removed.

Artifacts
---------

Also writes (when enabled):
- `.git/filter-repo/report.txt` via `--write-report` with counts for blobs stripped by size/SHA and blobs modified by `--replace-text`.

- `.git/filter-repo/commit-map`: old commit id (original-oid) -> new commit id.
- `.git/filter-repo/ref-map`: old ref -> new ref for tag/branch renames.

Limitations (prototype)
-----------------------

- No regex path matching; glob/prefix only.
- Merge simplification not implemented; we preserve merges but don't trim extra parents.
- No `--state-branch` yet; marks exported to a file.
- Windows path policy is always "sanitize" for rebuilt lines (no skip/error modes yet).

Examples
--------

- Literal message replacement:

  ```sh
  echo "FOO==>BAR" > replacements.txt
  rust/target/release/filter-repo-rs --replace-message replacements.txt
  ```

- Literal blob redaction:

  ```sh
  echo "SECRET_TOKEN==>REDACTED" > redact.txt
  rust/target/release/filter-repo-rs --replace-text redact.txt
  ```

- Regex blob redaction:

  ```sh
  echo "regex:api_key-[0-9]+==>REDACTED" > redact.txt
  rust/target/release/filter-repo-rs --replace-text redact.txt
  ```

- Write a report for stripped/modified blobs:

  ```sh
  rust/target/release/filter-repo-rs --max-blob-size 1024 --write-report
  cat .git/filter-repo/report.txt
  ```

- Run cleanup after import:

  ```sh
  rust/target/release/filter-repo-rs --cleanup standard
  # or
  rust/target/release/filter-repo-rs --cleanup aggressive
  ```
