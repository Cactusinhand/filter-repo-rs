# filter-repo-rs

[English](README.md) | [中文](README.zh-CN.md)

filter-repo-rs is a Rust prototype implementation of [git-filter-repo](https://github.com/newren/git-filter-repo), designed for efficiently rewriting Git repository history.

**Main Features:**

- 🚀 **High-performance streaming processing**: Based on `git fast-export` → filter → `git fast-import` pipeline architecture
- 🔒 **Sensitive data cleanup**: Safely remove API keys, passwords, and other sensitive information from commit history
- 📁 **Flexible path operations**: Support directory restructuring, file deletion, bulk renaming, and more
- 🏷️ **Reference management**: Intelligent handling of branch and tag renaming and migration
- 💾 **Safe backup mechanism**: Automatic backup of original history with full recovery support
- 🔍 **Repository analysis tools**: Check repository health, identify large files and potential issues

**Core Use Cases:**

- Completely remove accidentally committed sensitive information (keys, tokens, passwords, etc.) from version history
- Reduce repository size by removing large files, improving clone and operation performance
- Restructure directory layout, extract subdirectories, or adjust project organization
- Bulk rename branches and tags to standardize naming conventions
- Perform repository health checks and compliance verification in CI/CD pipelines

**⚠️ Project Status:** This is a prototype project under active development. While core functionality is stable, some advanced features are still being refined. Thorough testing is recommended before production use.

> To quickly understand this tool, please see typical usage scenarios:

## Typical Usage Scenarios

1. Mistakenly committed keys/tokens in history (API_TOKEN, SECRET, etc.)

- Goal: Remove sensitive strings from all commit history (including file contents and optionally commit messages), covering all refs.
- Recommended workflow:
  1. Backup current history first (strongly recommended):
     ```sh
     filter-repo-rs --backup
     ```
  2. Write content replacement rules (supports both literal and regex):
     ```sh
     # redact.txt
     SECRET_TOKEN==>REDACTED
     regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED
     ```
  3. Perform sensitive data cleanup on all refs (use --sensitive for comprehensive coverage including remote refs):
     ```sh
     filter-repo-rs \
       --sensitive \
       --replace-text redact.txt \
       --write-report
     ```
  4. If commit/tag messages also contain sensitive data, prepare separate message replacement rules (supports literal and regex via lines starting with `regex:`):
     ```sh
     filter-repo-rs --replace-message msg_rules.txt
     ```
  5. Force-push rewritten history:
     ```sh
     git push --force --all
     git push --force --tags
     ```
  6. Coordinate with team/CI to clean downstream forks/clone caches to prevent old history from returning.

2. Sensitive information in commit/tag messages needs cleanup

- Prepare message replacement rules (literal or regex):
  ```sh
  # messages.txt
  password==>[removed]
  ```
- Execute:
  ```sh
  filter-repo-rs --replace-message messages.txt --write-report
  ```
- Can be combined with `--backup`, `--sensitive`, `--dry-run` for safe rehearsal and comprehensive coverage.

3. Repository bloated due to large files/binary files, needs to slim down

- First analyze size and large object distribution:
  ```sh
  filter-repo-rs --analyze        # human readable
  filter-repo-rs --analyze --analyze-json   # machine readable
  ```
- Remove oversized objects by threshold (and delete corresponding paths):
  ```sh
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```
- `--max-blob-size` also supports human-readable suffixes like `5M`, `2G`.
- Or remove specific objects based on analysis results by listing OID manifest:
  ```sh
  filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
  ```
- Recommend moving large media to Git LFS or external storage to avoid future bloat.

4. Bulk renaming of tags/branches

- Tag prefix migration:
  ```sh
  filter-repo-rs --tag-rename v1.:legacy/v1.
  ```
- Branch prefix migration:

  ```sh
  filter-repo-rs --branch-rename feature/:exp/
  ```

- Combined usage: tag rename prefix + tag message rewrite (annotated tags are deduplicated and emitted once):

  ```sh
  # messages.txt contains literal replacements for commit/tag messages
  # e.g., café==>CAFE and 🚀==>ROCKET
  filter-repo-rs \
    --tag-rename orig-:renamed- \
    --replace-message messages.txt
  ```

- Combined usage: branch rename prefix + tag message rewrite (HEAD is automatically updated to new branch if the pointed branch is renamed):
  ```sh
  filter-repo-rs \
    --branch-rename original-:renamed- \
    --replace-message messages.txt
  ```

5. Adjust repository directory structure

- Extract subdirectory as new root (similar to splitting a module from monorepo):
  ```sh
  filter-repo-rs --subdirectory-filter frontend
  ```
- Move existing root to subdirectory:
  ```sh
  filter-repo-rs --to-subdirectory-filter app/
  ```
- Bulk path prefix renaming:
  ```sh
  filter-repo-rs --path-rename old/:new/
  ```

6. Remove specific files from history

- Remove specific file from all history (e.g., accidentally committed sensitive file):

  ```sh
  # 1. Backup first (strongly recommended)
  filter-repo-rs --backup

  # 2. Dry-run to verify operation
  filter-repo-rs \
    --path docs/STATUS.md \
    --invert-paths \
    --dry-run \
    --write-report

  # 3. Execute removal operation
  filter-repo-rs \
    --path docs/STATUS.md \
    --invert-paths \
    --write-report

  # 4. Force-push new history
  git push --force --all
  git push --force --tags
  ```

- Remove files matching patterns:
  ```sh
  filter-repo-rs --path-glob "*.log" --invert-paths
  ```
- Remove files using regex patterns:
  ```sh
  filter-repo-rs --path-regex "^temp/.*\.tmp$" --invert-paths
  ```

7. Safe execution recommendations and common switches

- Dry-run without persisting: `--dry-run`
- Generate audit report: `--write-report`
- Auto-backup before rewriting: `--backup [--backup-path PATH]`
- Sensitive mode (cover all remote refs): `--sensitive` (use with `--no-fetch` to skip fetching)
- Rewrite local only, skip remote cleanup: `--partial` (note: passing `--refs` implies `--partial`)
- Bypass protections when necessary: `--force` (use with caution)

8. Health analysis alerts in CI

- Execute in CI:
  ```sh
  filter-repo-rs --analyze --analyze-json
  ```
- Configure thresholds in a `.filter-repo-rs.toml` at repo root (preferred over legacy CLI flags):
  ```toml
  [analyze]
  top = 10

  [analyze.thresholds]
  warn_blob_bytes = 10_000_000
  warn_commit_msg_bytes = 4096
  warn_max_parents = 8
  ```
- For compatibility, legacy flags like `--analyze-large-blob` are gated behind `--debug-mode`/`FRRS_DEBUG=1` and emit deprecation warnings. See `docs/CLI-CONVERGENCE.zh-CN.md`.

## Quick Start

## Requirements

- Git available on PATH (recent version recommended)
- Rust toolchain (stable)
- Linux/macOS/Windows supported

## Build

### 单平台构建

```sh
cargo build -p filter-repo-rs --release
```

### 多平台交叉编译

我们提供了多种方式进行多平台编译：

#### 方法一：使用构建脚本（推荐）

**Linux/macOS:**
```sh
# 构建所有平台
./scripts/build-cross.sh

# 构建特定平台
./scripts/build-cross.sh x86_64-unknown-linux-gnu
./scripts/build-cross.sh x86_64-apple-darwin aarch64-apple-darwin
```

**Windows:**
```cmd
REM 构建所有平台
scripts\build-cross.bat

REM 构建特定平台
scripts\build-cross.bat x86_64-pc-windows-msvc
```

#### 方法二：手动交叉编译

首先安装 cross 工具：
```sh
cargo install cross --git https://github.com/cross-rs/cross
```

然后构建特定目标：
```sh
# Linux
cross build --target x86_64-unknown-linux-gnu --release -p filter-repo-rs
cross build --target aarch64-unknown-linux-gnu --release -p filter-repo-rs

# macOS (需要在 macOS 上运行或配置相应工具链)
cargo build --target x86_64-apple-darwin --release -p filter-repo-rs
cargo build --target aarch64-apple-darwin --release -p filter-repo-rs

# Windows
cross build --target x86_64-pc-windows-msvc --release -p filter-repo-rs
```

#### 支持的目标平台

| 平台 | 架构 | 目标标识符 | 说明 |
|------|------|------------|------|
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | 标准 Linux (glibc) |
| Linux | ARM64 | `aarch64-unknown-linux-gnu` | ARM64 Linux (glibc) |
| Linux | x86_64 | `x86_64-unknown-linux-musl` | 静态链接 Linux |
| Linux | ARM64 | `aarch64-unknown-linux-musl` | 静态链接 ARM64 Linux |
| macOS | x86_64 | `x86_64-apple-darwin` | Intel Mac |
| macOS | ARM64 | `aarch64-apple-darwin` | Apple Silicon Mac |
| Windows | x86_64 | `x86_64-pc-windows-msvc` | Windows (MSVC) |
| Windows | ARM64 | `aarch64-pc-windows-msvc` | Windows ARM64 |

构建产物将保存在 `target/releases/` 目录中。

## Testing

```sh
cargo test -p filter-repo-rs
```

- Unit tests are located within `src/` modules; integration tests are under `filter-repo-rs/tests/`, exercising the complete export→filter→import pipeline via public APIs.
- Tests create temporary Git repositories (no network required) and write debug artifacts (commit-map, ref-map, report) under `.git/filter-repo/`.

Run in Git repository (or pass `--source`/`--target`):

```sh
filter-repo-rs \
  --source . \
  --target . \
  --replace-message replacements.txt
```

## Backup and Recovery

`--backup` creates timestamped bundles under `.git/filter-repo/` by default.

Recovery method:

```sh
git clone /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle restored-repo
# or
git init restored-repo && cd restored-repo
git bundle unbundle /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle
git symbolic-ref HEAD refs/heads/<branch-from-bundle>
```

## Artifacts

- `.git/filter-repo/commit-map`: old commit → new commit
- `.git/filter-repo/ref-map`: old reference → new reference
- `.git/filter-repo/report.txt`: removal/modification counts and sample paths (when `--write-report` enabled)
- `.git/filter-repo/target-marks`: marks mapping table
- `.git/filter-repo/fast-export.filtered`: git fast-export filtered output (always)
- `.git/filter-repo/fast-export.original`: git fast-export original output (for debugging/reporting/size sampling)
- `.git/filter-repo/1758125153-834782600.bundle`: backup file

## Limitations and Notes

### Current Limitations

- Merge simplification strategy is still being optimized, complex topology scenarios may require manual handling
- Incremental processing (`--state-branch`) not yet supported
- Windows path policy fixed to "sanitize" mode

### Usage Recommendations

- Always use `--backup` to create backups before operating on large repositories
- Use `--dry-run` for rehearsal of sensitive operations
- Team coordination required to clean downstream caches when collaborating to prevent old history from returning
- Recommend validation on test repositories before production use

## Roadmap

### Near-term Plan (v0.1)

- [x] Basic streaming pipeline architecture
- [x] Path filtering and renaming
- [x] Content and message replacement
- [x] Branch and tag management
- [x] Backup and recovery mechanism

### Medium-term Planning (v0.2)

- [ ] Incremental processing support (`--state-branch`)
- [ ] Mailmap identity rewriting
- [ ] Merge simplification strategy optimization
- [ ] LFS integration and detection
- [ ] Windows path policy options

### Long-term Goals (v1.0)

- [ ] Performance benchmarking and optimization
- [ ] Complete internationalization support
- [ ] Graphical interface tools
- [ ] Plugin system architecture

## Contributing Guide

We welcome all forms of contributions! Whether bug reports, feature suggestions, code contributions, or documentation improvements.

### 🐛 Issue Reports

If you find bugs or have feature suggestions, please:

1. Check [Issues](../../issues) to confirm the issue hasn't been reported
2. Create a new issue using the provided template
3. Provide detailed reproduction steps and environment information
4. If possible, provide a minimal test case

### 💻 Code Contributions

1. **Fork this repository** and create your feature branch
2. **Follow code standards**: Run `cargo fmt` and `cargo clippy`
3. **Add tests**: Ensure new features have corresponding test cases
4. **Update documentation**: Include code comments and user documentation
5. **Submit Pull Request**: Clearly describe the changes and reasons

### 📝 Documentation Contributions

- Improve README and usage guides
- Supplement API documentation and code comments
- Translate documentation to other languages
- Provide usage examples and best practices

## Acknowledgments

### 🙏 Special Thanks

This project is deeply inspired by **[git-filter-repo](https://github.com/newren/git-filter-repo)**, an excellent Python project developed by [Elijah Newren](https://github.com/newren). `git-filter-repo` provides a powerful and flexible solution for Git repository history rewriting, and our Rust implementation extensively borrows from the wisdom of the original project in design philosophy and feature characteristics.

**Original Project Features:**

- 🎯 Mature and stable production-grade tool
- 🔧 Rich functionality and callback APIs
- 📚 Comprehensive documentation and community support
- 🏆 Official Git-recommended history rewriting tool

We recommend users choose the appropriate tool based on specific needs:

- **Choose git-filter-repo (Python)** if you need maximum feature completeness and ecosystem support
- **Choose filter-repo-rs (Rust)** if you value performance, memory safety, and modern language features

## License

This project is open source under the [MIT License](LICENSE).

## Contact

- **Project Homepage**: [GitHub Repository](https://github.com/cactusinhand/filter-repo-rs)
- **Issue Reports**: [Issues](../../issues)
- **Feature Requests**: [Discussions](../../discussions)
- **Security Issues**: Please contact via GitHub private reporting feature

---

<p align="center">
  <sub>Built with ❤️ and 🦀 by the Cactusinhand </sub>
</p>

<p align="center">
  <sub>If this project helps you, please consider giving us a ⭐️ Star</sub>
</p>
