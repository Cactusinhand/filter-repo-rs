Issue Checklist: CLI Convergence & Scope Guardrails
===================================================

This checklist tracks work items derived from SCOPE/STATUS/PARITY and the CLI convergence proposal.

Legend: [ ] todo, [~] in progress, [x] done

1) Gating & Help Layering
- [x] Add `--debug-mode` flag and/or `FRRS_DEBUG=1` env to expose hidden/debug flags in `--help`.
- [x] Split help output: default (core) vs verbose/debug (hidden).
- [x] Hide fast‑export low‑level flags (`--no-reencode`, `--no-quotepath`, `--mark-tags/--no-mark-tags`, `--date-order`) unless in debug.
- [x] Hide `--no-reset`, `--cleanup=aggressive` unless in debug.
- [x] Mark `--fe_stream_override` as test‑only (undocumented) unless in debug.

2) Cleanup Semantics
- [x] Replace `--cleanup [none|standard|aggressive]` with boolean `--cleanup` (standard), keep `--cleanup-aggressive` only in debug mode.
- [x] Ensure default (no flag) behaves same as current “none”.
- [x] Update finalize path to honor new flags; add tests.

3) Analysis Thresholds → Config File
- [x] Support `.filter-repo-rs.toml` config loading (repo root by default).
- [x] Map current CLI thresholds to config keys; CLI overrides config if provided.
- [x] Validate and error‑message on bad config; include example in docs.
- [x] Keep CLI: `--analyze`, `--analyze-json`, `--analyze-top` only.

4) Deprecation Strategy
- [x] Phase 1: accept old flags (thresholds, cleanup variants, etc.) with one‑time deprecation warnings + suggested replacements.
- [x] Phase 2: remove from `--help`, still accept with warnings.
- [ ] Phase 3: hard remove parsing (or keep only under `FRRS_DEBUG`).
- [x] Add migration guide snippets to docs/CLI-CONVERGENCE.zh-CN.md.

5) Defaults Review
- [ ] Confirm safe defaults: `reencode=yes`, `core.quotepath=false`, `--mark-tags`, topo order (no explicit `--date-order`).
- [ ] Add regression tests for defaults on Windows/Linux/macOS.

6) Docs & Examples
- [x] Update README.md / README.zh-CN.md CLI sections to reflect core set.
- [ ] Link SCOPE & PARITY prominently; add a “Quick recipes” subset.
- [x] Provide sample `.filter-repo-rs.toml` in docs and tests.
- [x] Update docs/STATUS.md with progress ticks.

7) Tests
- [x] Unit tests for gating (`--help`, debug exposure).
- [x] Integration tests for `--cleanup` semantics.
- [x] Config precedence tests (config vs CLI).
- [x] Backward‑compat acceptance of deprecated flags with warnings.

8) Release & Communication
- [ ] Changelog entry summarizing convergence, deprecations, and config migration.
- [ ] Tag a milestone and label issues/PRs accordingly.

Cross‑refs
----------
- docs/SCOPE.md, docs/SCOPE.zh-CN.md (scope & priorities)
- docs/PARITY.md (parity and non‑goals)
- docs/CLI-CONVERGENCE.zh-CN.md (proposal)
- docs/STATUS.md (roll‑up status)

