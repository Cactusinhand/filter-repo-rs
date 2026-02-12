# Testing Policy

This repository treats testing as a release gate, not a best effort.

## Definition Of Done

A change is not done until all of the following are true:

1. A failing test exists first for behavior changes (TDD red phase).
2. The implementation is minimal and makes that test pass (green phase).
3. Existing relevant tests still pass (regression check).
4. CLI-facing behavior is covered by behavior-style integration tests (BDD style).

## Required Test Types

Use the smallest combination that fully proves the change:

- Unit tests: pure logic and parsing behavior.
- Integration tests: end-to-end CLI behavior under `filter-repo-rs/tests/`.
- Regression tests: every bugfix adds at least one test that would fail before the fix.
- Stress/memory tests: critical resource paths and repeated operations.

## TDD Workflow (Mandatory For Feature/Bugfix Changes)

1. Write a failing test first.
2. Run only that test and confirm it fails for the expected reason.
3. Implement the smallest fix.
4. Re-run the focused test, then the relevant suite.
5. Before merge, run full regression.

If a change is pure refactor with no behavior change, prove equivalence with existing tests and add coverage only when a gap is found.

## BDD Style For CLI Tests

Integration tests should be scenario-oriented:

- Name tests by outcome/behavior, not implementation details.
- Prefer explicit setup + action + assertion flow.
- Cover both success and failure modes for user-facing flags and workflows.

Recommended naming style:

- `given_<context>_when_<action>_then_<outcome>`
- Or equivalent behavior sentence style already used in this repo.

## Commands

From repository root:

```bash
# Fast local signal
cargo test -p filter-repo-rs --lib

# Focused integration area
cargo test -p filter-repo-rs --test stream

# Memory/stress suite (stable mode)
cargo test -p filter-repo-rs --test memory -- --test-threads=1

# Full regression
cargo test --workspace --all-targets -- --test-threads=1

# CI-parity full regression (integration suites isolated)
cargo test --workspace --lib --bins -- --test-threads=1
for test_file in filter-repo-rs/tests/*.rs; do
  suite="$(basename "$test_file" .rs)"
  cargo test -p filter-repo-rs --test "$suite" -- --test-threads=1
done
```

## Flaky Test Policy

Flaky tests are treated as defects:

1. Reproduce and capture failure mode.
2. Fix root cause or isolate deterministic environment behavior.
3. Do not merge by silently skipping unstable coverage.

Test helper code may include bounded retries for transient system-level process spawn errors, but must never hide assertion failures or application errors.
