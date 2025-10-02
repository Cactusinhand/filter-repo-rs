# filter-repo-rs

[English](README.md) | [中文](README.zh-CN.md)

filter-repo-rs 是 [git-filter-repo](https://github.com/newren/git-filter-repo) 的 Rust 原型实现，用于高效地重写 Git 仓库历史。

**主要特性：**

- 🚀 **高性能流式处理**：基于 `git fast-export` → 过滤器 → `git fast-import` 的管道架构
- 🔒 **敏感数据清理**：从提交历史中安全移除 API 密钥、密码等敏感信息
- 📁 **灵活的路径操作**：支持目录重构、文件删除、批量重命名等操作
- 🏷️ **引用管理**：智能处理分支和标签的重命名与迁移
- 💾 **安全备份机制**：自动备份原始历史，支持完整恢复
- 🔍 **仓库分析工具**：检查仓库健康度，识别大文件和潜在问题

**核心用途：**

- 从版本历史中彻底清除意外提交的敏感信息（密钥、令牌、密码等）
- 通过移除大文件来减小仓库体积，提升克隆和操作性能
- 重构目录结构，提取子目录或调整项目布局
- 批量重命名分支和标签，规范命名约定
- 在 CI/CD 中进行仓库健康度检查和合规性验证

**⚠️ 项目状态：** 这是一个原型项目，正在积极开发中。虽然核心功能已经稳定，但某些高级特性仍在完善。建议在生产环境使用前进行充分测试。

> 为了快速立即这个工具，请看典型的使用场景：

## 典型使用场景

1. 历史记录中误提交了密钥/令牌（API_TOKEN、SECRET 等）

- 目标：从所有提交历史中清除敏感字串（包含文件内容与可选的提交说明），覆盖所有 refs。
- 建议流程：
  1. 先备份当前历史（强烈推荐）：
     ```sh
     filter-repo-rs --backup
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
  4. 如提交/标签消息中也包含敏感数据，另备一份消息替换规则（支持字面值与正则，正则规则以 `regex:` 开头）：
     ```sh
     filter-repo-rs --replace-message msg_rules.txt
     ```
  5. 重写历史后需要强制推送：
     ```sh
     git push --force --all
     git push --force --tags
     ```
  6. 与团队/CI 协调，清理下游 fork/clone 缓存，防止旧历史回流。

2. 提交/标签消息里有敏感信息，需要清洗

- 准备一份消息替换规则（可用字面值或正则）：
  ```sh
  # messages.txt
  password==>[removed]
  ```
- 执行：
  ```sh
  filter-repo-rs --replace-message messages.txt --write-report
  ```
- 可与 `--backup`、`--sensitive`、`--dry-run` 搭配以安全预演与全量覆盖。

3. 仓库因大文件/二进制文件膨胀，需要瘦身

- 先分析体积与大对象分布：
  ```sh
  filter-repo-rs --analyze        # 人类可读
  filter-repo-rs --analyze --analyze-json   # 机器可读
  ```
- 直接按阈值移除超大对象（并删除对应路径）：
  ```sh
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```
- `--max-blob-size` 同样支持 `5M`、`2G` 这类带后缀的可读格式。
- 或基于分析结果列出 OID 清单后定点移除：
  ```sh
  filter-repo-rs --strip-blobs-with-ids big-oids.txt --write-report
  ```
- 建议将大媒体转移至 Git LFS 或外部存储，避免后续再次膨胀。

4. 批量重命名标签/分支

- 标签前缀迁移：
  ```sh
  filter-repo-rs --tag-rename v1.:legacy/v1.
  ```
- 分支前缀迁移：

  ```sh
  filter-repo-rs --branch-rename feature/:exp/
  ```

- 组合用法：标签改名前缀 + 标签消息重写（注解标签会被去重并仅发射一次）

  ```sh
  # messages.txt 为提交/标签消息的字面值替换规则
  # 例如：café==>CAFE 与 🚀==>ROCKET
  filter-repo-rs \
    --tag-rename orig-:renamed- \
    --replace-message messages.txt
  ```

- 组合用法：分支改名前缀 + 标签消息重写（若 HEAD 所指分支被重命名，会自动更新到新分支）
  ```sh
  filter-repo-rs \
    --branch-rename original-:renamed- \
    --replace-message messages.txt
  ```

5. 调整仓库目录结构

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

6. 从历史中删除特定文件

- 从所有历史中删除特定文件（如意外提交的敏感文件）：

  ```sh
  # 1. 先备份（强烈推荐）
  filter-repo-rs --backup

  # 2. 干运行验证操作
  filter-repo-rs \
    --path docs/STATUS.md \
    --invert-paths \
    --dry-run \
    --write-report

  # 3. 执行删除操作
  filter-repo-rs \
    --path docs/STATUS.md \
    --invert-paths \
    --write-report

  # 4. 强制推送新历史
  git push --force --all
  git push --force --tags
  ```

- 删除匹配模式的文件：
  ```sh
  filter-repo-rs --path-glob "*.log" --invert-paths
  ```
- 使用正则表达式删除文件：
  ```sh
  filter-repo-rs --path-regex "^temp/.*\.tmp$" --invert-paths
  ```

7. 安全执行建议与常用开关

- 预演不落盘：`--dry-run`
- 产出审计报告：`--write-report`
- 重写前自动备份：`--backup [--backup-path PATH]`
- 敏感模式（覆盖所有远端引用）：`--sensitive`（配合 `--no-fetch` 可跳过抓取）
- 仅重写本地、跳过远端清理：`--partial`（注意：传入 `--refs` 等价于隐式开启 `--partial`）
- 必要时跳过保护：`--force`（谨慎使用）

8. CI 中的健康度分析预警

- 在 CI 里执行：
  ```sh
  filter-repo-rs --analyze --analyze-json
  ```
- 将阈值配置到仓库根目录的 `.filter-repo-rs.toml`（优先于旧式 CLI 旗标）：
  ```toml
  [analyze]
  top = 10

  [analyze.thresholds]
  warn_blob_bytes = 10_000_000
  warn_commit_msg_bytes = 4096
  warn_max_parents = 8
  ```
- 兼容期内，`--analyze-large-blob` 等旧旗标需要 `--debug-mode`/`FRRS_DEBUG=1`，并会打印弃用告警。参见 `docs/CLI-CONVERGENCE.zh-CN.md`。

## 快速开始

## 环境要求

- PATH 中可用的 Git（建议较新版本）
- Rust 工具链（stable）
- 支持 Linux/macOS/Windows

## 构建

```sh
cargo build -p filter-repo-rs --release
```

## 测试

```sh
cargo test -p filter-repo-rs
```

- 单元测试位于 `src/` 模块内；集成测试位于 `filter-repo-rs/tests/`，以公开 API 跑通完整的导出 → 过滤 → 导入。
- 测试会创建临时 Git 仓库（无需联网），并在其 `.git/filter-repo/` 下写入调试产物（commit-map、ref-map、report）。

在 Git 仓库中运行（或传入 `--source`/`--target`）：

```sh
filter-repo-rs \
  --source . \
  --target . \
  --replace-message replacements.txt
```

## 备份与恢复

`--backup` 默认在 `.git/filter-repo/` 下创建带时间戳的 bundle。

恢复方式：

```sh
git clone /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle restored-repo
# 或者
git init restored-repo && cd restored-repo
git bundle unbundle /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle
git symbolic-ref HEAD refs/heads/<branch-from-bundle>
```

## 产物

- `.git/filter-repo/commit-map`：旧提交 → 新提交
- `.git/filter-repo/ref-map`：旧引用 → 新引用
- `.git/filter-repo/report.txt`：剔除/修改计数及示例路径（启用 `--write-report` 时）
- `.git/filter-repo/target-marks`: marks 映射表
- `.git/filter-repo/fast-export.filtered`: git fast-export 被过滤后的输出（始终）
- `.git/filter-repo/fast-export.original`: git fast-export 原输出（调试/报告/体积采样时）
- `.git/filter-repo/1758125153-834782600.bundle`: 备份文件

## 限制与注意事项

### 当前限制

- 合并简化策略仍在优化中，复杂拓扑场景可能需要手动处理
- 暂不支持增量处理（`--state-branch`）
- Windows 路径策略固定为 "sanitize" 模式

### 使用建议

- 大型仓库操作前务必使用 `--backup` 创建备份
- 敏感操作建议先用 `--dry-run` 预演
- 团队协作时需协调清理下游缓存，防止旧历史回流
- 生产环境使用前建议在测试仓库上验证

## 路线图

### 近期计划 (v0.1)

- [x] 基础流式管道架构
- [x] 路径过滤与重命名
- [x] 内容与消息替换
- [x] 分支标签管理
- [x] 备份恢复机制

### 中期规划 (v0.2)

- [ ] 增量处理支持 (`--state-branch`)
- [ ] Mailmap 身份重写
- [ ] 合并简化策略优化
- [ ] LFS 集成与检测
- [ ] Windows 路径策略选项

### 长期目标 (v1.0)

- [ ] 性能基准测试与优化
- [ ] 完整的国际化支持
- [ ] 图形界面工具
- [ ] 插件系统架构

## 贡献指南

我们欢迎各种形式的贡献！无论是错误报告、功能建议、代码贡献还是文档改进。

### 🐛 问题报告

如果您发现 bug 或有功能建议，请：

1. 检查 [Issues](../../issues) 确认问题未被报告
2. 使用提供的模板创建新 issue
3. 提供详细的复现步骤和环境信息
4. 如果可能，提供最小化的测试用例

### 💻 代码贡献

1. **Fork 本仓库**并创建您的功能分支
2. **遵循代码规范**：运行 `cargo fmt` 和 `cargo clippy`
3. **添加测试**：确保新功能有对应的测试用例
4. **更新文档**：包括代码注释和用户文档
5. **提交 Pull Request**：描述清楚变更内容和原因

### 📝 文档贡献

- 改进 README 和使用指南
- 补充 API 文档和代码注释
- 翻译文档到其他语言
- 提供使用示例和最佳实践

## 致谢

### 🙏 特别感谢

本项目深受 **[git-filter-repo](https://github.com/newren/git-filter-repo)** 的启发，这是一个由 [Elijah Newren](https://github.com/newren) 开发的优秀 Python 项目。`git-filter-repo` 为 Git 仓库历史重写提供了强大而灵活的解决方案，我们的 Rust 实现在设计理念和功能特性上大量借鉴了原项目的智慧。

**原项目特点：**

- 🎯 成熟稳定的生产级工具
- 🔧 丰富的功能和回调 API
- 📚 完善的文档和社区支持
- 🏆 Git 官方推荐的历史重写工具

我们建议用户根据具体需求选择合适的工具：

- **选择 git-filter-repo（Python）** 如果您需要最大的功能完整性和生态支持
- **选择 filter-repo-rs（Rust）** 如果您看重性能、内存安全和现代语言特性

## 许可证

本项目采用 [MIT 许可证](LICENSE) 开源。

## 联系方式

- **项目主页**: [GitHub 仓库](https://github.com/cactusinhand/filter-repo-rs)
- **问题报告**: [Issues](../../issues)
- **功能请求**: [Discussions](../../discussions)
- **安全问题**: 请通过 GitHub 私有报告功能联系

---

<p align="center">
  <sub>Built with ❤️ and 🦀 by the Cactusinhand </sub>
</p>

<p align="center">
  <sub>如果本项目对您有帮助，请考虑给我们一个 ⭐️ Star</sub>
</p>
