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

Testing
-------

Integration tests exercise the end-to-end pipeline, spawning temporary Git
repositories under `target/it/`. They cover path filtering and prefix renames,
commit/ref map generation, Windows path sanitization policy, and final ref
topology checks. Run them with:

```
cargo test -p filter-repo-rs
```

The suite requires Git in `PATH` and writes debug artifacts (commit-map,
ref-map, reports) inside each ephemeral repository for verification.

Key Flags (prototype)
---------------------

### Repository & ref selection

- `--source DIR`, `--target DIR`: working directories (default `.`)
- `--ref|--refs REF`: repeatable; defaults to `--all`
- `--date-order`, `--no-data`: pass-through to `git fast-export`

### Path selection & rewriting

- `--path PREFIX`: include-only by prefix (repeatable; ORed)
- `--path-glob GLOB`: include by glob (supports `*`, `?`, `**`; repeatable; ORed)
- `--path-regex REGEX`: include by regex (Rust `regex` crate in bytes mode; repeatable; ORed)
- `--invert-paths`: invert selection (drop matches; keep others)
- `--path-rename OLD:NEW`: rename path prefix in file changes
- `--subdirectory-filter DIR`: equivalent to `--path DIR/ --path-rename DIR/:`
- `--to-subdirectory-filter DIR`: equivalent to `--path-rename :DIR/`

Regex path filters do not support look-around or backreferences (crate limitation). Prefer anchored patterns when scanning
large histories to minimize backtracking costs.

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

Use Cases (典型使用场景)
------------------------

1) 历史记录中误提交了密钥/令牌（API_TOKEN、SECRET 等）

- 目标：从所有提交历史中清除敏感字串（包含文件内容与可选的提交说明），覆盖所有 refs。
- 建议流程：
  1. 先备份当前历史（强烈推荐）：
     ```sh
     filter-repo-rs --backup --refs --all
     ```
  2. 编写内容替换规则（支持字面值与正则）：
     ```sh
     # redact.txt
     SECRET_TOKEN==>REDACTED
     regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED
     ```
  3. 对所有 refs 进行敏感数据清洗（包含远端 refs 时可用 --sensitive 进行全量覆盖）：
     ```sh
     filter-repo-rs \
       --sensitive \
       --replace-text redact.txt \
       --write-report
     ```
  4. 如提交信息（commit message）中也包含敏感数据，另备一份消息替换规则并加上：
     ```sh
     filter-repo-rs --replace-message msg_rules.txt
     ```
  5. 重写历史后需要强制推送：
     ```sh
     git push --force --all
     git push --force --tags
     ```
  6. 与团队/CI 协调，清理下游 fork/clone 缓存，防止旧历史回流。

2) 提交说明（commit message）里有敏感信息，需要清洗

- 准备一份消息替换规则：
  ```sh
  # messages.txt
  password==>[removed]
  regex:token=[0-9A-Za-z_\-]+==>[removed]
  ```
- 执行：
  ```sh
  filter-repo-rs --replace-message messages.txt --write-report
  ```
- 可与 `--backup`、`--sensitive`、`--dry-run` 搭配以安全预演与全量覆盖。

3) 仓库因大文件/二进制文件膨胀，需要瘦身

- 先分析体积与大对象分布：
  ```sh
  filter-repo-rs --analyze        # 人类可读
  filter-repo-rs --analyze --analyze-json   # 机器可读
  ```
- 直接按阈值移除超大对象（并删除对应路径）：
  ```sh
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```
- 或基于分析结果列出 OID 清单后定点移除：
  ```sh
  filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
  ```
- 建议将大媒体转移至 Git LFS 或外部存储，避免后续再次膨胀。

4) 批量重命名标签/分支

- 标签前缀迁移：
  ```sh
  filter-repo-rs --tag-rename v1.:legacy/v1.
  ```
- 分支前缀迁移：
  ```sh
  filter-repo-rs --branch-rename feature/:exp/
  ```

5) 调整仓库目录结构

- 提取子目录为新根（类似 monorepo 拆分某模块）：
  ```sh
  filter-repo-rs --subdirectory-filter frontend
  ```
- 将现有根移动到子目录：
  ```sh
  filter-repo-rs --to-subdirectory-filter app/
  ```
- 批量路径前缀改名：
  ```sh
  filter-repo-rs --path-rename old/:new/
  ```

6) 安全执行建议与常用开关

- 预演不落盘：`--dry-run`
- 产出审计报告：`--write-report`
- 重写前自动备份：`--backup [--backup-path PATH]`
- 敏感模式（覆盖所有远端引用）：`--sensitive`（配合 `--no-fetch` 可跳过抓取）
- 仅重写本地、跳过远端清理：`--partial`
- 必要时跳过保护：`--force`（谨慎使用）

7) CI 中的健康度分析预警

- 在 CI 里执行：
  ```sh
  filter-repo-rs --analyze --analyze-json \
    --analyze-large-blob 10_000_000 \
    --analyze-commit-msg-warn 4096 \
    --analyze-max-parents-warn 8
  ```
- 根据阈值输出 `warnings`，用于阻断超限提交或提醒库体积趋势。


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
