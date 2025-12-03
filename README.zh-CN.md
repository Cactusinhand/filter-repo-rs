# filter-repo-rs

[English](README.md) | [中文](README.zh-CN.md)

> 🦀 快速、安全的 Git 历史重写工具 — 清除密钥、瘦身仓库、重构路径。

## 解决什么问题？

| 😱 痛点                 | ✅ 一条命令                                             |
| ----------------------- | ------------------------------------------------------- |
| 密钥/Token 不小心提交了 | `filter-repo-rs --replace-text secrets.txt --sensitive` |
| 仓库太大，clone 半天    | `filter-repo-rs --max-blob-size 10M`                    |
| 想把子目录拆成独立仓库  | `filter-repo-rs --subdirectory-filter frontend`         |
| 批量改 tag/branch 前缀  | `filter-repo-rs --tag-rename v1.:legacy/v1.`            |
| 删除历史中的某个文件    | `filter-repo-rs --path docs/secret.md --invert-paths`   |
| 分析仓库健康度          | `filter-repo-rs --analyze`                              |

## 快速示例

### 清除泄露的密钥

```sh
# 1. 先备份（强烈推荐）
filter-repo-rs --backup

# 2. 编写替换规则 (secrets.txt)
#    API_KEY_12345==>REDACTED
#    regex:password\s*=\s*"[^"]+==>[REMOVED]

# 3. 清洗所有历史
filter-repo-rs --replace-text secrets.txt --sensitive --write-report

# 4. 强制推送
git push --force --all && git push --force --tags
```

### 仓库瘦身

```sh
# 先分析
filter-repo-rs --analyze

# 移除超过 10MB 的文件
filter-repo-rs --max-blob-size 10M --write-report
```

### 重构路径

```sh
# 提取子目录为新根
filter-repo-rs --subdirectory-filter src/frontend

# 将根目录移入子目录
filter-repo-rs --to-subdirectory-filter packages/core

# 批量重命名路径
filter-repo-rs --path-rename old/:new/
```

## 安全第一

| 参数             | 用途                     |
| ---------------- | ------------------------ |
| `--backup`       | 重写前创建带时间戳的备份 |
| `--dry-run`      | 预演，不实际修改         |
| `--write-report` | 生成变更审计报告         |
| `--sensitive`    | 覆盖所有 refs（含远端）  |

## 安装

**环境要求：** Git、Rust 工具链 (stable)、Linux/macOS/Windows

```sh
# 从源码构建
cargo build -p filter-repo-rs --release

# 产物位置: target/release/filter-repo-rs
```

<details>
<summary>交叉编译</summary>

```sh
# 使用构建脚本（推荐）
./scripts/build-cross.sh                    # 所有平台
./scripts/build-cross.sh x86_64-apple-darwin # 指定目标

# 或手动使用 cross
cargo install cross --git https://github.com/cross-rs/cross
cross build --target x86_64-unknown-linux-gnu --release -p filter-repo-rs
```

| 平台                | 目标                        |
| ------------------- | --------------------------- |
| Linux x64           | `x86_64-unknown-linux-gnu`  |
| Linux ARM64         | `aarch64-unknown-linux-gnu` |
| macOS Intel         | `x86_64-apple-darwin`       |
| macOS Apple Silicon | `aarch64-apple-darwin`      |
| Windows x64         | `x86_64-pc-windows-msvc`    |

</details>

## 全部场景

<details>
<summary>1. 清除文件内容中的敏感信息</summary>

```sh
# secrets.txt - 支持字面值和正则
SECRET_TOKEN==>REDACTED
regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED

filter-repo-rs --replace-text secrets.txt --sensitive --write-report
```

</details>

<details>
<summary>2. 清洗提交消息中的敏感信息</summary>

```sh
# messages.txt
password==>[removed]

filter-repo-rs --replace-message messages.txt --write-report
```

</details>

<details>
<summary>3. 移除大文件 / 仓库瘦身</summary>

```sh
# 按大小阈值
filter-repo-rs --max-blob-size 5M --write-report

# 按指定 blob ID
filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
```

</details>

<details>
<summary>4. 批量重命名 tag/branch</summary>

```sh
filter-repo-rs --tag-rename v1.:legacy/v1.
filter-repo-rs --branch-rename feature/:exp/
```

</details>

<details>
<summary>5. 重构目录结构</summary>

```sh
# 提取子目录为新根
filter-repo-rs --subdirectory-filter frontend

# 将根移入子目录
filter-repo-rs --to-subdirectory-filter app/

# 重命名路径前缀
filter-repo-rs --path-rename old/:new/
```

</details>

<details>
<summary>6. 从历史中删除特定文件</summary>

```sh
# 单个文件
filter-repo-rs --path docs/STATUS.md --invert-paths

# 按 glob 模式
filter-repo-rs --path-glob "*.log" --invert-paths

# 按正则
filter-repo-rs --path-regex "^temp/.*\.tmp$" --invert-paths
```

</details>

<details>
<summary>7. CI 健康检查</summary>

```sh
filter-repo-rs --analyze --analyze-json
```

在 `.filter-repo-rs.toml` 配置阈值：

```toml
[analyze.thresholds]
warn_blob_bytes = 10_000_000
warn_commit_msg_bytes = 4096
```

</details>

## 备份与恢复

```sh
# 备份产物: .git/filter-repo/backup-YYYYMMDD-HHMMSS.bundle
filter-repo-rs --backup

# 恢复
git clone /path/to/backup.bundle restored-repo
```

## 产物

运行后查看 `.git/filter-repo/`：

- `commit-map` — 旧提交 → 新提交映射
- `ref-map` — 旧引用 → 新引用映射
- `report.txt` — 变更摘要（需 `--write-report`）

## 限制

- 合并简化策略仍在优化，复杂拓扑可能需手动处理
- 暂不支持增量处理（`--state-branch`）
- Windows 路径策略固定为 "sanitize" 模式

## 致谢

本项目受 [git-filter-repo](https://github.com/newren/git-filter-repo)（[Elijah Newren](https://github.com/newren) 开发）启发 — Git 官方推荐的历史重写工具。

- **选择 git-filter-repo** — 需要最完整的功能
- **选择 filter-repo-rs** — 看重性能和内存安全

## 许可证

[MIT](LICENSE)

## 链接

- [GitHub](https://github.com/Cactusinhand/filter-repo-rs)
- [Issues](https://github.com/Cactusinhand/filter-repo-rs/issues)
- [Discussions](https://github.com/Cactusinhand/filter-repo-rs/discussions)

---

<p align="center">
  <sub>Built with ❤️ and 🦀 by Cactusinhand</sub>
</p>
