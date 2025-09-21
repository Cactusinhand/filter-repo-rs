---
name: CLI convergence (consolidation & gating)
about: Track a cohesive set of CLI convergence tasks
title: "CLI convergence: consolidate flags, gating, config migration"
labels: ["cli", "convergence", "scope"]
assignees: []
---

## Checklist

- [ ] Add `--debug-mode` and/or `FRRS_DEBUG=1` to expose hidden/debug flags in `--help`.
- [ ] Help layering: default (core) vs verbose/debug (hidden) output.
- [ ] Hide fast‑export low‑level flags unless debug: `--no-reencode`, `--no-quotepath`, `--mark-tags/--no-mark-tags`, `--date-order`.
- [ ] Hide `--no-reset` and keep `--cleanup-aggressive` only under debug.
- [ ] Make `--cleanup` boolean (standard), wire finalize path.
- [ ] Config support for analysis thresholds (`.filter-repo-rs.toml`), CLI overrides config.
- [ ] Deprecation phase 1: accept old flags with warnings + suggestions.
- [ ] Deprecation phase 2: remove from `--help`, still accept with warnings.
- [ ] Deprecation phase 3: remove parsing (or gate under debug).
- [ ] Documentation: update README(s), STATUS, SCOPE; add sample config.
- [ ] Tests: gating/help, cleanup semantics, config precedence, deprecated flag acceptance.

## Notes

Link to:

- docs/SCOPE.md, docs/SCOPE.zh-CN.md
- docs/PARITY.md
- docs/CLI-CONVERGENCE.zh-CN.md
- docs/STATUS.md

