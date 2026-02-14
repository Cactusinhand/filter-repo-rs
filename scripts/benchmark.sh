#!/bin/bash
# Performance benchmark: measure filter-repo-rs performance

REPO=${1:-perf-test-repo-1000}
FILTERED="${REPO}-filtered"
FILTER_REPO_BIN="./target/release/filter-repo-rs"

echo "=== Performance Benchmark ==="
echo "Repository: $REPO"
echo ""

# Check if repo exists
if [ ! -d "$REPO/.git" ]; then
    echo "Error: Repository $REPO not found"
    echo "Run: ./scripts/create-perf-test-repo.sh 1000"
    exit 1
fi

# Check if binary exists
if [ ! -f "$FILTER_REPO_BIN" ]; then
    echo "Building release binary..."
    cargo build --release -p filter-repo-rs
fi

echo "--- Test 1: Analysis Mode ---"
echo "Command: filter-repo-rs --analyze"
/usr/bin/time -l $FILTER_REPO_BIN --analyze 2>&1 | tee /tmp/analyze_output.txt
echo ""

echo "--- Test 2: Full Filter (dry-run) ---"
echo "Command: filter-repo-rs --dry-run"
/usr/bin/time -l $FILTER_REPO_BIN --source "$REPO" --target "$FILTERED" --dry-run 2>&1 | tee /tmp/filter_output.txt
echo ""

echo "--- Memory Analysis ---"
echo "Peak memory (analysis):"
grep "maximum resident" /tmp/analyze_output.txt | head -1
echo ""
echo "Peak memory (filter):"
grep "maximum resident" /tmp/filter_output.txt | head -1
echo ""

echo "=== Done ==="
