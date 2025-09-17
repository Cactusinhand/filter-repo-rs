filter-repo-rs (Rust Prototype)
===============================

This is a Rust reimplementation prototype of git-filter-repo. It streams
`git fast-export` -> filters -> `git fast-import`, keeps debug streams, and provides
several core features with Windows compatibility.

Build
-----

cargo build -p filter-repo-rs --release

Run
---

Run from inside a Git repository (or pass `--source`/`--target`):

filter-repo-rs \
  --source . \
  --target . \
  --refs --all \
  --date-order \
  --replace-message replacements.txt

Key Flags (prototype)
---------------------

### Repository & ref selection

- `--source DIR`, `--target DIR`: working directories (default `.`)
- `--ref|--refs REF`: repeatable; defaults to `--all`
- `--date-order`, `--no-data`: pass-through to `git fast-export`

### Path selection & rewriting

- `--path PREFIX`: include-only by prefix (repeatable; ORed)
- `--path-glob GLOB`: include by glob (supports `*`, `?`, `**`; repeatable; ORed)
- `--invert-paths`: invert selection (drop matches; keep others)
- `--path-rename OLD:NEW`: rename path prefix in file changes
- `--subdirectory-filter DIR`: equivalent to `--path DIR/ --path-rename DIR/:`
- `--to-subdirectory-filter DIR`: equivalent to `--path-rename :DIR/`

### Blob filtering & redaction

- `--replace-text FILE`: literal replacements applied to blob contents (files). Same syntax
  as `--replace-message`. Lines starting with `regex:` are treated as regex rules
  (e.g., `regex:foo[0-9]+==>X`). Enabled in the default build.
- `--max-blob-size BYTES`: drop blobs larger than BYTES and delete paths that reference them.
- `--strip-blobs-with-ids FILE`: drop blobs whose 40-hex ids (one per line) are listed.

### Commit, tag & ref updates

- `--replace-message FILE`: literal replacements for commit/tag messages.
  Each non-empty, non-comment line is `from==>to` or `from` (implies `***REMOVED***`).
- `--tag-rename OLD:NEW`: rename tags starting with OLD to start with NEW
- `--branch-rename OLD:NEW`: rename branches starting with OLD to start with NEW

### Execution behavior & output

- `--write-report`: write a summary to `.git/filter-repo/report.txt`.
- `--cleanup [none|standard|aggressive]`: post-import cleanup (reflog expire + gc). Default `none`.
- `--quiet`, `--no-reset`: reduce noise / skip post-import reset
- `--no-reencode`, `--no-quotepath`, `--no-mark-tags`: pass-through fast-export toggles
- `--backup`: create a git bundle of the selected refs under `.git/filter-repo/` (skipped in `--dry-run`).
- `--backup-path PATH`: override where the bundle is written (directory or explicit file path).

### Restoring from bundle backups

When `--backup` runs, the tool invokes `git bundle create` with a file name such as
`backup-20240216-153012-123456789.bundle`. The timestamp is recorded in UTC down to
nanoseconds so repeated runs cannot collide, and the `.bundle` extension matches what
`git bundle` expects.

To recover a repository from one of these backups:

1. Create a new directory (it does not need to be a Git repository yet).
2. Clone the bundle into that directory:

   ```sh
   git clone /path/to/backup-20240216-153012-123456789.bundle restored-repo
   ```

   Alternatively, to import into an existing empty repository, run:

   ```sh
   git init restored-repo
   cd restored-repo
   git bundle unbundle /path/to/backup-20240216-153012-123456789.bundle
   git symbolic-ref HEAD refs/heads/<branch-from-bundle>
   ```

3. Inspect the restored refs (e.g., `git show-ref`) and continue working from the recovered history.

### Safety & advanced modes

- `--partial`: partial rewrite; disables origin migration, ref cleanup, reflog gc.
- `--sensitive` (aka sensitive-data removal): enables fetch-all refs to ensure coverage; implies skipping origin removal.
- `--no-fetch`: do not fetch refs even in `--sensitive` mode.
- `--force`, `-f`: bypass sanity checks (danger: destructive).
- `--enforce-sanity`: enable preflight safety checks.
- `--dry-run`: do not update refs or clean up; preview only.

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
  filter-repo-rs --replace-message replacements.txt
  ```

- Literal blob redaction:

  ```sh
  echo "SECRET_TOKEN==>REDACTED" > redact.txt
  filter-repo-rs --replace-text redact.txt
  ```

- Regex blob redaction:

  ```sh
  echo "regex:api_key-[0-9]+==>REDACTED" > redact.txt
  filter-repo-rs --replace-text redact.txt
  ```

- Write a report for stripped/modified blobs:

  ```sh
  filter-repo-rs --max-blob-size 1024 --write-report
  cat .git/filter-repo/report.txt
  ```

- Run cleanup after import:

  ```sh
  filter-repo-rs --cleanup standard
  # or
  filter-repo-rs --cleanup aggressive
  ```
