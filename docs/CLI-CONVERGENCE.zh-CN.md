CLI 收敛草案（filter-repo-rs）
=============================

目标与原则
--------

- 最小可用表面：面向 80% 用户的少量高价值开关即可完成常见任务。
- 安全默认优先：不暴露易误用的低层旗标，提升默认行为的“正确性”。
- 渐进暴露：进阶/调试开关通过“调试模式”启用；阈值细节移至配置文件。
- 兼容过渡：对需要变更的旗标，提供“接受但告警”的缓冲期与迁移文档。

一、核心 CLI（保留并文档化）
---------------------------

- 仓库与引用
  - `--source DIR`、`--target DIR`、`--refs`（可重复，默认 `--all`）
  - `--sensitive`（覆盖全部 refs）＋`--no-fetch`（可选跳过抓取）
  - `--partial`（仅重写本地，跳过远端迁移/移除）
- 路径与重构
  - `--path`、`--path-glob`、`--path-regex`、`--invert-paths`
  - `--path-rename OLD:NEW`、`--subdirectory-filter DIR`、`--to-subdirectory-filter DIR`
- 内容与对象
  - `--replace-text FILE`（文件内容：字节/regex 规则）
  - `--max-blob-size BYTES`、`--strip-blobs-with-ids FILE`
- 消息与引用
  - `--replace-message FILE`（字面值；哈希自动回写保留）
  - `--tag-rename OLD:NEW`、`--branch-rename OLD:NEW`
- 执行与产物
  - `--backup [--backup-path PATH]`、`--write-report`、`--dry-run`、`--quiet`
  - `--cleanup`（布尔，等价当前的 standard；见“合并与语义调整”）

二、合并与语义调整（建议）
-------------------------

- `--cleanup [none|standard|aggressive]` → 简化为：
  - `--cleanup`（开启标准清理：`reflog expire --expire=now --all` + `git gc --prune=now --quiet`）
  - 如需 aggressive，仅在调试模式提供隐藏开关（见“三、隐藏/调试开关”）。
- `--no-reset` → 不对外文档化
  - `--dry-run` 已满足“不落盘”；`--no-reset` 仅在调试模式可用。

三、隐藏/调试开关（默认不在帮助中显示）
-------------------------------------

- fast-export 细节：`--no-reencode`、`--no-quotepath`、`--mark-tags/--no-mark-tags`、`--date-order`
  - 作用：兼容/调试底层差异；默认采用安全值（reencode=yes、quotepath=false、mark-tags、拓扑顺序）。
- 行为控制：`--no-reset`、`--cleanup=aggressive`
  - 仅调试/性能试验需要，默认不建议。
- 流覆盖：`--fe_stream_override`
  - 用于测试从文件注入 fast-export 流；对终端用户隐藏。

开启方式（建议）：
- `FRRS_DEBUG=1 filter-repo-rs …` 或 `--debug-mode` 时，在 `--help` 中显示上述隐藏开关。

四、分析参数收敛与配置文件
-------------------------

- 面向 CLI 保留：`--analyze`、`--analyze-json`、`--analyze-top N`
- 将阈值微调迁移到配置文件（默认 `.filter-repo-rs.toml`）：

```toml
[analyze]
top = 10

[analyze.thresholds]
warn_total_bytes = 1073741824        # 1 GiB
crit_total_bytes = 5368709120        # 5 GiB
warn_blob_bytes = 10485760           # 10 MiB
warn_ref_count = 20000
warn_object_count = 10000000
warn_tree_entries = 2000
warn_path_length = 200
warn_duplicate_paths = 1000
warn_commit_msg_bytes = 10000
warn_max_parents = 8
```

- CLI 若显式传入阈值，则覆盖配置文件；否则按配置/默认。旧 CLI 阈值旗标进入弃用阶段（见下文“迁移指南”）。

五、弃用与兼容策略
-------------------

- 阶段 1（N 个小版本）：接受旧旗标并打印一次性告警，提示等效新语义或配置项。
- 阶段 2：从 `--help` 移除旧旗标；仍接受但保持告警。
- 阶段 3：移除解析（或仅在 `FRRS_DEBUG` 下保留）。

示例映射：
- `--cleanup aggressive` → 在调试模式使用 `--cleanup-aggressive`（隐藏）；常规场景推荐仅 `--cleanup`。
- `--no-reset` → 由 `--dry-run` 覆盖；调试模式可显式使用。
- 分析阈值族 → 对应配置文件键值；CLI 仅保留 `--analyze(-json/top)`。

迁移指南（阶段 1 → 阶段 2）：

| 旧旗标/语法 | 推荐替代 | 告警文案摘要 |
| --- | --- | --- |
| `--cleanup=none` | 直接省略 `--cleanup`，保持默认无清理 | `warning: --cleanup=none is deprecated; simply omit --cleanup to keep cleanup disabled.` |
| `--cleanup=standard` | 使用布尔型 `--cleanup` | `warning: --cleanup=standard is deprecated; use --cleanup (boolean) to request standard cleanup.` |
| `--cleanup=aggressive` | 在调试模式使用 `--cleanup-aggressive` | `warning: --cleanup=aggressive is deprecated; use --cleanup-aggressive in debug mode if you need the old aggressive behavior.` |
| `--analyze-total-warn BYTES` | `analyze.thresholds.warn_total_bytes` | `warning: --analyze-total-warn is deprecated; set analyze.thresholds.warn_total_bytes in your .filter-repo-rs.toml (or --config) file instead.` |
| `--analyze-total-critical BYTES` | `analyze.thresholds.crit_total_bytes` | 同上模式，指向相应配置键 |
| `--analyze-large-blob BYTES` | `analyze.thresholds.warn_blob_bytes` | 同上模式 |
| `--analyze-ref-warn COUNT` | `analyze.thresholds.warn_ref_count` | 同上模式 |
| `--analyze-object-warn COUNT` | `analyze.thresholds.warn_object_count` | 同上模式 |
| `--analyze-tree-entries N` | `analyze.thresholds.warn_tree_entries` | 同上模式 |
| `--analyze-path-length N` | `analyze.thresholds.warn_path_length` | 同上模式 |
| `--analyze-duplicate-paths N` | `analyze.thresholds.warn_duplicate_paths` | 同上模式 |
| `--analyze-commit-msg-warn N` | `analyze.thresholds.warn_commit_msg_bytes` | 同上模式 |
| `--analyze-max-parents-warn N` | `analyze.thresholds.warn_max_parents` | 同上模式 |

> 以上告警仅会打印一次，并附带 `note: see docs/CLI-CONVERGENCE.zh-CN.md for the analysis threshold migration table.`，便于脚本在阶段 1/2 内更新。

六、帮助文本分层
----------------

- 常规 `--help`：仅显示“一、核心 CLI”。
- `--help --verbose` 或 `FRRS_DEBUG=1`：追加“三、隐藏/调试开关”。

七、用户影响与迁移指南（摘要）
----------------------------

- 绝大多数常见场景（历史清洗/瘦身/路径重构/标签分支改名/审计）不受影响，参数更少更易记。
- 依赖阈值细节的团队：将现有阈值转写到 `.filter-repo-rs.toml`；命令行改用 `--analyze/--analyze-json/--analyze-top`。
- 依赖低层开关的高级用户：通过 `FRRS_DEBUG=1` 暴露原有调试旗标；或按建议等效迁移。

八、后续工作
------------

- 在 `--help` 与 README 中体现新分层；STATUS/SCOPE 追踪推进状态。
- 代码层面：
  - 新增 `--debug-mode` 或 `FRRS_DEBUG` 环境变量的 gating。
  - 读取 `.filter-repo-rs.toml`（解析错误应提供友好提示与示例）。
  - 打印弃用告警与等效建议（阶段 1/2）。

