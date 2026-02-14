# Performance Testing Framework

This directory contains tools for benchmarking and performance testing of filter-repo-rs.

## Quick Start

```bash
# Run a quick performance test (small repo)
python3 scripts/perf_test.py

# Run with specific scenario
python3 scripts/perf_test.py --scenario medium

# Run multiple test types
python3 scripts/perf_test.py --scenario small --test analyze --test dryrun
```

## Scripts

| Script | Description |
|--------|-------------|
| `perf_test.py` | Main Python test framework |
| `memory_profile.sh` | Memory profiling script |
| `create-perf-test-repo.sh` | Create test repositories |
| `benchmark.sh` | Basic benchmark script |

## Test Scenarios

| Scenario | Commits | Description |
|----------|---------|-------------|
| tiny | 100 | Quick smoke test |
| small | 1,000 | Development testing |
| medium | 5,000 | CI/QA testing |
| large | 10,000 | Performance testing |
| huge | 50,000 | Stress testing |

## Test Types

| Test | Description |
|------|-------------|
| analyze | Repository analysis only |
| dryrun | Full filter with --dry-run |
| partial | Partial rewrite with path filter |

## Usage Examples

### Running Tests

```bash
# Run all scenarios with analyze test
python3 scripts/perf_test.py --scenario all --test analyze

# Run specific scenario with multiple tests
python3 scripts/perf_test.py --scenario medium --test analyze --test dryrun

# Clean up test repos after
python3 scripts/perf_test.py --scenario small --clean
```

### Memory Profiling

```bash
# Profile memory usage
./scripts/memory_profile.sh /path/to/repo
```

### Manual Testing

```bash
# Build release binary
cargo build --release -p filter-repo-rs

# Time analysis
/usr/bin/time -l ./target/release/filter-repo-rs --analyze --source /path/to/repo

# Time filter
/usr/bin/time -l ./target/release/filter-repo-rs --source /path/to/repo --target /path/to/filtered
```

## Results

Test results are saved to:
- `scripts/perf_results/perf_results_*.csv`

CSV format includes:
- timestamp
- scenario
- repo_commits
- repo_size_mb
- test_name
- duration_sec
- peak_memory_mb
- exit_code
- success

## Comparing Results

```bash
# Compare two runs
python3 scripts/perf_test.py --compare
```

## Troubleshooting

### Out of memory

If tests fail due to memory:
1. Use smaller scenarios (tiny/small)
2. Increase system swap
3. Profile memory with Instruments (macOS)

### Test repo creation fails

Ensure you have:
- Git installed
- Sufficient disk space (~1GB per 10k commits)
- Write permissions to /tmp

## Adding New Tests

To add new test scenarios, edit `perf_test.py`:

```python
# Add new scenario size
SIZES = {
    ...
    "custom": 50000,
}

# Add new test type
if "mype" in tests:
    tester.run_test(repo_path, "mytest", ["--myargs"], scenario)
```
