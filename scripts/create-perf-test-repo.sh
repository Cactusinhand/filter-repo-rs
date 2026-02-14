#!/bin/bash
# Performance test: create a large test repository

set -e

REPO_SIZE=${1:-1000}  # Number of commits
REPO_NAME="perf-test-repo-$REPO_SIZE"

echo "Creating test repository with $REPO_SIZE commits..."

# Clean up
rm -rf "$REPO_NAME"
mkdir "$REPO_NAME"
cd "$REPO_NAME"
git init

# Create initial commit
mkdir -p src
echo "initial content" > src/main.rs
git add .
git commit -m "Initial commit"

# Create multiple commits
for i in $(seq 1 $REPO_SIZE); do
    echo "content $i" >> src/file_$((i % 10)).txt
    git add .
    git commit -m "Commit $i" --quiet
    if [ $((i % 100)) -eq 0 ]; then
        echo "  Created $i commits..."
    fi
done

# Add some large blobs
echo "Adding large blobs..."
for i in $(seq 1 5); do
    dd if=/dev/urandom of=large_file_$i.bin bs=1M count=10 2>/dev/null
    git add .
    git commit -m "Add large file $i" --quiet
done

echo "Repository created: $REPO_NAME"
echo "  Commits: $(git rev-list --count --all)"
echo "  Size: $(du -sh .git | cut -f1)"
