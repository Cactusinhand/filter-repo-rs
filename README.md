# filter-repo-rs

[English](README.md) | [中文](README.zh-CN.md)

> 🦀 Fast, safe Git history rewriting in Rust — remove secrets, slim repos, restructure paths.

## What Problems Does It Solve?

| 😱 Your Problem                          | ✅ One Command                                          |
| ---------------------------------------- | ------------------------------------------------------- |
| Leaked API keys/tokens in history        | `filter-repo-rs --replace-text secrets.txt --sensitive` |
| Repo too large, clone takes forever      | `filter-repo-rs --max-blob-size 10M`                    |
| Need to extract subdirectory as new repo | `filter-repo-rs --subdirectory-filter frontend`         |
| Bulk rename tags/branches                | `filter-repo-rs --tag-rename v1.:legacy/v1.`            |
| Remove specific file from all history    | `filter-repo-rs --path docs/secret.md --invert-paths`   |
| Normalize author/committer identities    | `filter-repo-rs --mailmap .mailmap`                     |
| Analyze repo health                      | `filter-repo-rs --analyze`                              |

## Quick Examples

### Remove Leaked Secrets

```sh
# 1. Backup first (strongly recommended)
filter-repo-rs --backup

# 2. Create replacement rules (secrets.txt)
#    API_KEY_12345==>REDACTED
#    regex:password\s*=\s*"[^"]+==>[REMOVED]

# 3. Clean all history
filter-repo-rs --replace-text secrets.txt --sensitive --write-report

# 4. Force push
git push --force --all && git push --force --tags
```

### Slim Down Bloated Repo

```sh
# Analyze first
filter-repo-rs --analyze

# Remove files larger than 10MB
filter-repo-rs --max-blob-size 10M --write-report
```

### Restructure Paths

```sh
# Extract subdirectory as new root
filter-repo-rs --subdirectory-filter src/frontend

# Move root into subdirectory
filter-repo-rs --to-subdirectory-filter packages/core

# Bulk rename paths
filter-repo-rs --path-rename old/:new/
```

### Rewrite Author/Committer Identities

```sh
# Option A: Use .mailmap style rules
# Format: New Name <new@email> <old@email>
filter-repo-rs --mailmap .mailmap --write-report

# Option B: Use explicit rewrite files
# author.txt / committer.txt: oldName==>newName
# email.txt: oldEmail==>newEmail
filter-repo-rs --author-rewrite author.txt \
  --committer-rewrite committer.txt \
  --email-rewrite email.txt \
  --write-report
```

Note: `--mailmap` takes precedence. If `--mailmap` is provided, `--author-rewrite`,
`--committer-rewrite`, and `--email-rewrite` are ignored for identity lines.

## Safety First

| Flag             | Purpose                                    |
| ---------------- | ------------------------------------------ |
| `--backup`       | Create timestamped bundle before rewriting |
| `--dry-run`      | Preview changes without modifying anything |
| `--write-report` | Generate audit report of all changes       |
| `--sensitive`    | Cover all refs including remotes           |

## Installation

**Requirements:** Git on PATH, Rust toolchain (stable), Linux/macOS/Windows

```sh
# Build from source
cargo build -p filter-repo-rs --release

# Binary at: target/release/filter-repo-rs
```

<details>
<summary>Cross-platform builds</summary>

```sh
# Using build script (recommended)
./scripts/build-cross.sh                    # All platforms
./scripts/build-cross.sh x86_64-apple-darwin # Specific target

# Or manually with cross
cargo install cross --git https://github.com/cross-rs/cross
cross build --target x86_64-unknown-linux-gnu --release -p filter-repo-rs
```

| Platform            | Target                      |
| ------------------- | --------------------------- |
| Linux x64           | `x86_64-unknown-linux-gnu`  |
| Linux ARM64         | `aarch64-unknown-linux-gnu` |
| macOS Intel         | `x86_64-apple-darwin`       |
| macOS Apple Silicon | `aarch64-apple-darwin`      |
| Windows x64         | `x86_64-pc-windows-msvc`    |

</details>

## All Use Cases

<details>
<summary>1. Remove secrets from file contents</summary>

```sh
# secrets.txt - supports literal and regex
SECRET_TOKEN==>REDACTED
regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED

filter-repo-rs --replace-text secrets.txt --sensitive --write-report
```

</details>

<details>
<summary>2. Clean sensitive commit messages</summary>

```sh
# messages.txt
password==>[removed]

filter-repo-rs --replace-message messages.txt --write-report
```

</details>

<details>
<summary>3. Rewrite author/committer identity history</summary>

```sh
# .mailmap
Jane Doe <jane@company.com> <jane@users.noreply.github.com>

filter-repo-rs --mailmap .mailmap --write-report
```

```sh
# author-rules.txt / committer-rules.txt
old name==>new name

# email-rules.txt
old@email.com==>new@email.com

filter-repo-rs --author-rewrite author-rules.txt \
  --committer-rewrite committer-rules.txt \
  --email-rewrite email-rules.txt \
  --write-report
```

</details>

<details>
<summary>4. Remove large files / slim repo</summary>

```sh
# By size threshold
filter-repo-rs --max-blob-size 5M --write-report

# By specific blob IDs
filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
```

</details>

<details>
<summary>5. Rename tags/branches in bulk</summary>

```sh
filter-repo-rs --tag-rename v1.:legacy/v1.
filter-repo-rs --branch-rename feature/:exp/
```

</details>

<details>
<summary>6. Restructure directory layout</summary>

```sh
# Extract subdirectory as new root
filter-repo-rs --subdirectory-filter frontend

# Move root to subdirectory
filter-repo-rs --to-subdirectory-filter app/

# Rename path prefixes
filter-repo-rs --path-rename old/:new/
```

</details>

<details>
<summary>7. Remove specific files from history</summary>

```sh
# Single file
filter-repo-rs --path docs/STATUS.md --invert-paths

# By glob pattern
filter-repo-rs --path-glob "*.log" --invert-paths

# By regex
filter-repo-rs --path-regex "^temp/.*\.tmp$" --invert-paths
```

</details>

<details>
<summary>8. CI health checks</summary>

```sh
filter-repo-rs --analyze --analyze-json
```

Configure thresholds in `.filter-repo-rs.toml`:

```toml
[analyze.thresholds]
warn_blob_bytes = 10_000_000
warn_commit_msg_bytes = 4096
```

</details>

## Backup & Recovery

```sh
# Backup creates: .git/filter-repo/backup-YYYYMMDD-HHMMSS.bundle
filter-repo-rs --backup

# Restore
git clone /path/to/backup.bundle restored-repo
```

## Artifacts

After running, check `.git/filter-repo/`:

- `commit-map` — old → new commit mapping
- `ref-map` — old → new reference mapping
- `report.txt` — change summary (with `--write-report`)

## Limitations

- Merge simplification still being optimized for complex topologies
- No incremental processing (`--state-branch`) yet
- Windows path policy fixed to "sanitize" mode

## Acknowledgments

Inspired by [git-filter-repo](https://github.com/newren/git-filter-repo) by [Elijah Newren](https://github.com/newren) — the official Git-recommended history rewriting tool.

- **Choose git-filter-repo** for maximum feature completeness
- **Choose filter-repo-rs** for performance and memory safety

## License

[MIT](LICENSE)

## Links

- [GitHub](https://github.com/Cactusinhand/filter-repo-rs)
- [Issues](https://github.com/Cactusinhand/filter-repo-rs/issues)
- [Discussions](https://github.com/Cactusinhand/filter-repo-rs/discussions)
- [Testing Policy](TESTING.md)

---

<p align="center">
  <sub>Built with ❤️ and 🦀 by Cactusinhand</sub>
</p>
