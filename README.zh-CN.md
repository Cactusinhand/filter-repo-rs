filter-repo-rs（git-filter-repo 的 Rust 原型）
=============================================

filter-repo-rs 是 [git-filter-repo](https://github.com/newren/git-filter-repo) 的 Rust 原型实现。

它通过 `git fast-export` → 进程内过滤 → `git fast-import` 的流式管线工作，
输出调试流，并强调跨平台（含 Windows）的安全与性能。

- [English](README.md) | [中文](README.zh-CN.md)

状态：原型。尚未完全等同 Python 版，但已可用于常见场景。

为了快速立即这个工具，请看典型的使用场景：

典型使用场景
------------

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
  4. 如提交/标签消息中也包含敏感数据，另备一份消息替换规则（当前仅字面值）：
     ```sh
     filter-repo-rs --replace-message msg_rules.txt
     ```
  5. 重写历史后需要强制推送：
     ```sh
     git push --force --all
     git push --force --tags
     ```
  6. 与团队/CI 协调，清理下游 fork/clone 缓存，防止旧历史回流。

2) 提交/标签消息里有敏感信息，需要清洗

- 准备一份消息替换规则（当前仅字面值）：
  ```sh
  # messages.txt
  password==>[removed]
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

快速开始
--------

在 Git 仓库中运行（或传入 `--source`/`--target`）：

```sh
filter-repo-rs \
  --source . \
  --target . \
  --refs --all \
  --date-order \
  --replace-message replacements.txt
```

特性
----

- 流式 pipeline
  - `fast-export` → 过滤器 → `fast-import`，调试副本写至 `.git/filter-repo/`。
  - 已启用的 fast-export 选项：`--show-original-ids`、`--signed-tags=strip`、
    `--tag-of-filtered-object=rewrite`、`--fake-missing-tagger`、
    `--reference-excluded-parents`、`--use-done-feature`。
  - `fast-import` 使用 `-c core.ignorecase=false`，marks 输出至 `.git/filter-repo/target-marks`。

- 路径选择与重写
  - 支持按前缀 `--path`、glob `--path-glob`（`*`、`?`、`**`）或正则 `--path-regex`（Rust regex，不支持环视/反向引用）。
  - `--invert-paths` 反转选择；`--path-rename OLD:NEW` 执行前缀重命名。
  - 便捷项：`--subdirectory-filter DIR`、`--to-subdirectory-filter DIR`。

- Blob 过滤与脱敏
  - `--replace-text FILE` 替换文件内容；支持字面值与 `regex:` 规则（如 `regex:api_key-[0-9]+==>REDACTED`）。
  - `--max-blob-size BYTES` 移除超大 blob，并删除引用它们的路径。
  - `--strip-blobs-with-ids FILE` 移除文件中列出的 40 十六进制 blob。

- 提交/标签/引用
  - `--replace-message FILE` 对提交/标签消息执行字面值替换。
  - 自动将消息中的旧提交短/长哈希重写为新哈希（借助生成的 `commit-map`）。
  - `--tag-rename`、`--branch-rename` 基于前缀重命名；注解标签去重后仅发射一次。
  - 非合并的空提交通过 `alias` 到其首个父标记而被剪枝；合并提交保留。
  - 导入后执行安全的引用更新与 HEAD 选择。

- 安全、备份与分析
  - 可选预检；`--backup` 重写前创建 bundle；`--write-report` 输出总结。
  - 分析模式：`--analyze`（人类可读）或 `--analyze --analyze-json`（机器可读）。

环境要求
--------

- PATH 中可用的 Git（建议较新版本）
- Rust 工具链（stable）
- 支持 Linux/macOS/Windows

构建
----

```sh
cargo build -p filter-repo-rs --release
```

测试
----

```sh
cargo test -p filter-repo-rs
```

测试会在 `target/it/` 下创建临时仓库；要求 PATH 中有 Git；
并在每个临时仓库里写入调试产物（commit-map、ref-map、report）。

命令概览（节选）
----------------

- 仓库与引用
  - `--source DIR`、`--target DIR`（默认 `.`）、`--refs`（可重复，默认 `--all`）
  - `--date-order`、`--no-data` 透传给 fast-export

- 路径
  - `--path`、`--path-glob`、`--path-regex`、`--invert-paths`
  - `--path-rename OLD:NEW`、`--subdirectory-filter DIR`、`--to-subdirectory-filter DIR`

- 内容与 blob
  - `--replace-text FILE`、`--max-blob-size BYTES`、`--strip-blobs-with-ids FILE`

- 消息与引用
  - `--replace-message FILE`、`--tag-rename OLD:NEW`、`--branch-rename OLD:NEW`

- 行为与输出
  - `--write-report`、`--cleanup [none|standard|aggressive]`、`--quiet`、`--no-reset`
  - `--no-reencode`、`--no-quotepath`、`--no-mark-tags`、`--mark-tags`
  - `--backup [--backup-path PATH]`、`--dry-run`
  - `--partial`、`--sensitive [--no-fetch]`、`--force`、`--enforce-sanity`

示例
----

- 清除历史中的敏感信息（文件内容）

  ```sh
  # 1) 备份（推荐）
  filter-repo-rs --backup --refs --all

  # 2) 编写替换规则
  cat > redact.txt <<EOF
  SECRET_TOKEN==>REDACTED
  regex:(API|TOKEN|SECRET)[A-Za-z0-9_-]+==>REDACTED
  EOF

  # 3) 执行并输出报告
  filter-repo-rs --sensitive --replace-text redact.txt --write-report

  # 4) 强制推送新历史
  git push --force --all && git push --force --tags
  ```

- 清洗提交/标签消息（字面值规则）

  ```sh
  cat > messages.txt <<EOF
  password==>[removed]
  EOF
  filter-repo-rs --replace-message messages.txt --write-report
  ```

- 通过移除大对象瘦身

  ```sh
  # 先分析
  filter-repo-rs --analyze
  filter-repo-rs --analyze --analyze-json

  # 移除 >5MB 的 blob，并删除对应路径
  filter-repo-rs --max-blob-size 5_000_000 --write-report
  ```

- 目录重构

  ```sh
  # 提取子目录为新根
  filter-repo-rs --subdirectory-filter frontend

  # 将当前根移动到子目录
  filter-repo-rs --to-subdirectory-filter app/

  # 批量路径前缀改名
  filter-repo-rs --path-rename old/:new/
  ```

备份与恢复
----------

`--backup` 默认在 `.git/filter-repo/` 下创建带时间戳的 bundle。

恢复方式：

```sh
git clone /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle restored-repo
# 或者
git init restored-repo && cd restored-repo
git bundle unbundle /path/to/backup-YYYYMMDD-HHMMSS-XXXXXXXXX.bundle
git symbolic-ref HEAD refs/heads/<branch-from-bundle>
```

功能要点
--------

- 调试流：`.git/filter-repo/fast-export.{original,filtered}`。
- 非合并空提交剪枝（`alias` 到首个父标记）；合并提交保留。
- 标签
  - 注解标签：缓冲、可改名、去重后仅发射一次。
  - 轻量标签：`reset`/`from` 配对缓冲，在 `done` 前刷新。
- 引用
  - 仅当新引用存在时删除旧引用；`ref-map` 记录重命名。
  - 尝试将 HEAD 更新到有效分支（优先映射后的分支）。
- 远端
  - 完整运行（非 `--partial`）前，将 `refs/remotes/origin/*` 迁移到 `refs/heads/*`。
  - 非敏感模式运行后移除 `origin`，避免误推旧历史；敏感模式可抓取所有引用（除非 `--no-fetch`），且保留 `origin`。

产物
----

- `.git/filter-repo/commit-map`：旧提交 → 新提交
- `.git/filter-repo/ref-map`：旧引用 → 新引用
- `.git/filter-repo/report.txt`：剔除/修改计数及示例路径（启用 `--write-report` 时）
- `.git/filer-repo/target-marks`: marks 映射表
- `.git/filter-repo/fast-export.original`: git fast-export 原输出
- `.git/filter-repo/fast-export.filtered`: git fast-export 被过滤后的输出
- `.git/filter-repo/1758125153-834782600.bundle`: 备份文件

Windows 注意
-----------

- 重新构建的路径会进行 Windows 兼容化（保留字替换、去除结尾点/空格）。
- 部分备份测试可能受 MSYS/Cygwin 路径转换影响，见 tests/README 的规避方法。

限制（原型）
-----------

- 未实现合并简化；尚未剪枝退化合并。
- 尚无 `--state-branch`（仅导出 marks 到文件）。
- Windows 路径策略固定为 “sanitize”（暂无 skip/error）。
- 尚无回调 API 与基于 mailmap 的身份重写。
- `--replace-message` 仅支持字面值规则；正则支持计划中。
- 已启用短哈希重写；`--preserve-commit-hashes` 开关计划中。
- 尚未支持人类可读大小（如 `5M`）。

路线图 / TODO（与 Python 版对齐）
------------------------------

- 路径能力：`--paths-from-file`、`--use-base-name`、`--path-rename-match`/正则改名
- 消息：`--replace-message` 支持 `regex:`；`--preserve-commit-hashes`
- 大小参数：支持 `5M`/`2G` 等，并提供 `--strip-blobs-bigger-than` 别名
- 身份：mailmap（`--mailmap`、`--use-mailmap`）与姓名/邮箱回调
- 合并：在保证祖先正确性的前提下裁剪退化合并
- replace-refs 与增量：`--replace-refs …`、`--state-branch`、stash（`refs/stash`）重写
- 分析与报告：LFS 相关输出；更丰富的产物
- Windows 路径策略：`--windows-path-policy=[sanitize|skip|error]` 与报表
- 回调框架：filename/refname/blob/commit/tag/reset/message/name/email
- 安全默认：考虑默认开启更严格的预检；完善 partial/sensitive 文档
