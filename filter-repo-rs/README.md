# filter-repo-rs

Fast, safe Git history rewriting in Rust — remove secrets, slim repos, restructure paths.

> 🦀 A Rust implementation inspired by [git-filter-repo](https://github.com/newren/git-filter-repo).

## Documentation

- [Full Documentation](https://github.com/Cactusinhand/filter-repo-rs)
- [中文文档](https://github.com/Cactusinhand/filter-repo-rs/blob/main/README.zh-CN.md)

## Installation

```bash
cargo install filter-repo-rs
```

## Quick Start

```bash
# Analyze repository
filter-repo-rs --analyze

# Remove secrets from history
filter-repo-rs --replace-text secrets.txt --sensitive

# Slim down large files
filter-repo-rs --max-blob-size 10M
```

## License

MIT
