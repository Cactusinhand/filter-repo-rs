#!/bin/bash
# Memory profiling script for filter-repo-rs
# Uses dtrace on macOS or /usr/bin/time on Linux

set -e

REPO_PATH=${1:-"/tmp/test-repo"}
OUTPUT_DIR="perf_results/memory"
BINARY="./target/release/filter-repo-rs"

mkdir -p "$OUTPUT_DIR"

echo "=== Memory Profiling ==="
echo "Repository: $REPO_PATH"
echo "Output: $OUTPUT_DIR"
echo ""

# Check if repo exists
if [ ! -d "$REPO_PATH/.git" ]; then
    echo "Error: Repository not found at $REPO_PATH"
    exit 1
fi

# Build if needed
if [ ! -f "$BINARY" ]; then
    echo "Building release binary..."
    cargo build --release -p filter-repo-rs
fi

# Test scenarios
SCENARIOS=(
    "analyze:--analyze"
    "dryrun:--dry-run --target /tmp/filtered-test"
    "partial:--partial --paths README.md --target /tmp/filtered-test-partial"
)

for scenario in "${SCENARIOS[@]}"; do
    name="${scenario%%:*}"
    args="${scenario#*:}"
    
    echo "--- Testing: $name ---"
    echo "Args: $args"
    
    # Run with /usr/bin/time (macOS)
    output_file="$OUTPUT_DIR/memory_${name}.txt"
    
    /usr/bin/time -l "$BINARY" $args 2>&1 | tee "$output_file"
    
    # Extract key metrics
    echo "  Extracted metrics:"
    grep "maximum resident" "$output_file" || echo "    (no memory data)"
    grep "user time" "$output_file" || echo "    (no time data)"
    grep "system time" "$output_file" || echo "    (no time data)"
    echo ""
done

echo "=== Memory profiling complete ==="
echo "Results saved to: $OUTPUT_DIR/"
