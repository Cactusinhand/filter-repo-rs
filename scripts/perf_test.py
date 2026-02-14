#!/usr/bin/env python3
"""
Performance Test Framework for filter-repo-rs

This framework provides:
- Multiple test scenarios (small/medium/large/huge repos)
- Memory profiling with /usr/bin/time
- Execution time tracking
- CSV result storage
- Comparison reports

Usage:
    python3 scripts/perf_test.py                    # Run all tests
    python3 scripts/perf_test.py --scenario medium # Run specific scenario
    python3 scripts/perf_test.py --compare         # Compare results
"""

import argparse
import csv
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from datetime import datetime
from pathlib import Path
from typing import Optional, List, Dict, Any


# Configuration
SCRIPT_DIR = Path(__file__).parent
PROJECT_ROOT = SCRIPT_DIR.parent
TARGET_DIR = PROJECT_ROOT / "target" / "release"
BINARY_NAME = "filter-repo-rs"
RESULTS_DIR = SCRIPT_DIR / "perf_results"
TEST_REPO_BASE = "/tmp/filter-repo-perf-test"


@dataclass
class PerfResult:
    """Performance test result"""

    timestamp: str
    scenario: str
    repo_commits: int
    repo_size_mb: float
    test_name: str
    duration_sec: float
    peak_memory_mb: float
    exit_code: int
    success: bool
    notes: str = ""


class TestRepo:
    """Create test repositories with various sizes"""

    SIZES = {
        "tiny": 100,
        "small": 1000,
        "medium": 5000,
        "large": 10000,
        "huge": 50000,
    }

    def __init__(self, base_path: str = TEST_REPO_BASE):
        self.base_path = Path(base_path)

    def create(self, size: str) -> Path:
        """Create a test repository of given size"""
        commits = self.SIZES.get(size, 1000)
        repo_path = self.base_path / f"repo-{size}"

        if repo_path.exists():
            print(f"  Repository already exists: {repo_path}")
            return repo_path

        print(f"  Creating {size} repository ({commits} commits)...")
        repo_path.mkdir(parents=True, exist_ok=True)

        # Initialize git repo
        subprocess.run(["git", "init"], cwd=repo_path, check=True, capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "test@test.com"], cwd=repo_path, check=True
        )
        subprocess.run(
            ["git", "config", "user.name", "Test User"], cwd=repo_path, check=True
        )

        # Initial commit
        (repo_path / "README.md").write_text("# Test Repository\n")
        subprocess.run(["git", "add", "."], cwd=repo_path, check=True)
        subprocess.run(
            ["git", "commit", "-m", "Initial commit"], cwd=repo_path, check=True
        )

        # Create commits in batches
        batch_size = 100
        for i in range(0, commits, batch_size):
            batch_commits = min(batch_size, commits - i)
            for j in range(batch_commits):
                idx = i + j + 1
                (repo_path / f"file_{idx % 10}.txt").write_text(f"content {idx}\n")
                subprocess.run(
                    ["git", "add", "."], cwd=repo_path, check=True, capture_output=True
                )
                subprocess.run(
                    ["git", "commit", "-m", f"Commit {idx}", "--quiet"],
                    cwd=repo_path,
                    check=True,
                    capture_output=True,
                )

            print(f"    Progress: {i + batch_commits}/{commits} commits")

        print(f"  Created: {repo_path}")
        return repo_path

    def cleanup(self):
        """Remove all test repositories"""
        import shutil

        if self.base_path.exists():
            shutil.rmtree(self.base_path)
            print(f"  Cleaned up: {self.base_path}")


class PerfTester:
    """Run performance tests"""

    def __init__(self, binary_path: Path):
        self.binary = binary_path
        self.results: List[PerfResult] = []

    def run_test(
        self, repo_path: Path, test_name: str, args: List[str], scenario: str
    ) -> PerfResult:
        """Run a single performance test"""
        print(f"\n  Running: {test_name}")
        print(f"    Args: {' '.join(args)}")

        # Get repo stats
        result = subprocess.run(
            ["git", "rev-list", "--count", "--all"],
            cwd=repo_path,
            capture_output=True,
            text=True,
        )
        commits = int(result.stdout.strip()) if result.returncode == 0 else 0

        result = subprocess.run(
            ["du", "-sm", ".git"], cwd=repo_path, capture_output=True, text=True
        )
        size_mb = float(result.stdout.split()[0]) if result.returncode == 0 else 0

        # Run with time measurement
        start_time = time.time()

        cmd = [str(self.binary)] + args
        process = subprocess.Popen(
            cmd,
            cwd=repo_path,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Use /usr/bin/time to measure memory on macOS
        time_cmd = ["/usr/bin/time", "-l"] + cmd
        time_process = subprocess.run(
            time_cmd,
            cwd=repo_path,
            capture_output=True,
            text=True,
        )

        duration = time.time() - start_time
        exit_code = time_process.returncode

        # Parse memory from /usr/bin/time output
        peak_memory = 0.0
        for line in time_process.stderr.split("\n"):
            if "maximum resident set size" in line:
                # Convert bytes to MB
                peak_memory = float(line.split()[0]) / (1024 * 1024)
                break

        success = exit_code == 0

        result = PerfResult(
            timestamp=datetime.now().isoformat(),
            scenario=scenario,
            repo_commits=commits,
            repo_size_mb=size_mb,
            test_name=test_name,
            duration_sec=round(duration, 2),
            peak_memory_mb=round(peak_memory, 2),
            exit_code=exit_code,
            success=success,
        )

        print(
            f"    Duration: {result.duration_sec}s, Memory: {result.peak_memory_mb}MB"
        )

        self.results.append(result)
        return result

    def save_results(self, filename: Optional[str] = None):
        """Save results to CSV"""
        RESULTS_DIR.mkdir(exist_ok=True)

        if filename is None:
            filename = f"perf_results_{datetime.now().strftime('%Y%m%d_%H%M%S')}.csv"

        filepath = RESULTS_DIR / filename

        fieldnames = [
            "timestamp",
            "scenario",
            "repo_commits",
            "repo_size_mb",
            "test_name",
            "duration_sec",
            "peak_memory_mb",
            "exit_code",
            "success",
            "notes",
        ]

        with open(filepath, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=fieldnames)
            writer.writeheader()
            for r in self.results:
                writer.writerow(asdict(r))

        print(f"\n  Results saved to: {filepath}")
        return filepath

    def print_summary(self):
        """Print test summary"""
        print("\n" + "=" * 60)
        print("PERFORMANCE TEST SUMMARY")
        print("=" * 60)

        for scenario in set(r.scenario for r in self.results):
            print(f"\n{scenario.upper()}:")
            for r in self.results:
                if r.scenario == scenario:
                    status = "✓" if r.success else "✗"
                    print(
                        f"  {status} {r.test_name}: {r.duration_sec}s, {r.peak_memory_mb}MB"
                    )


def run_tests(scenarios: List[str], tests: List[str], clean: bool):
    """Run performance tests"""

    # Find binary
    binary = TARGET_DIR / BINARY_NAME
    if not binary.exists():
        print("Building release binary...")
        subprocess.run(
            ["cargo", "build", "--release", "-p", "filter-repo-rs"],
            cwd=PROJECT_ROOT,
            check=True,
        )

    # Create tester
    tester = PerfTester(binary)

    # Create test repos and run tests
    repo_creator = TestRepo()

    try:
        for scenario in scenarios:
            print(f"\n{'=' * 60}")
            print(f"Testing scenario: {scenario}")
            print(f"{'=' * 60}")

            # Create repo
            repo_path = repo_creator.create(scenario)

            # Run tests
            if "analyze" in tests:
                tester.run_test(repo_path, "analyze", ["--analyze"], scenario)

            if "dryrun" in tests:
                tester.run_test(
                    repo_path,
                    "dryrun",
                    ["--dry-run", "--force"],
                    scenario,
                )

            if "partial" in tests:
                target = repo_path.parent / f"{repo_path.name}-filtered-partial"
                tester.run_test(
                    repo_path,
                    "partial",
                    [
                        "--source",
                        str(repo_path),
                        "--target",
                        str(target),
                        "--partial",
                        "--paths",
                        "file_0.txt",
                    ],
                    scenario,
                )

    finally:
        # Cleanup repos if requested
        if clean:
            repo_creator.cleanup()

        # Save results
        tester.print_summary()
        tester.save_results()


def compare_results():
    """Compare previous test results"""
    import shutil

    if not RESULTS_DIR.exists():
        print("No results to compare")
        return

    csv_files = sorted(RESULTS_DIR.glob("perf_results_*.csv"))
    if not csv_files:
        print("No result files found")
        return

    print(f"Found {len(csv_files)} result files")

    # Load latest two files
    if len(csv_files) >= 2:
        latest = csv_files[-1]
        previous = csv_files[-2]

        print(f"\nComparing:")
        print(f"  Previous: {previous.name}")
        print(f"  Latest:   {latest.name}")

        # Simple comparison
        print("\n" + "=" * 60)
        print("COMPARISON")
        print("=" * 60)


def main():
    parser = argparse.ArgumentParser(description="Performance test framework")
    parser.add_argument(
        "--scenario",
        choices=["tiny", "small", "medium", "large", "huge", "all"],
        default="small",
        help="Test scenario",
    )
    parser.add_argument(
        "--test",
        dest="tests",
        action="append",
        choices=["analyze", "dryrun", "partial"],
        default=["analyze"],
        help="Tests to run",
    )
    parser.add_argument(
        "--clean", action="store_true", help="Clean up test repos after"
    )
    parser.add_argument("--compare", action="store_true", help="Compare results")

    args = parser.parse_args()

    if args.compare:
        compare_results()
        return

    # Determine scenarios
    scenarios = (
        [args.scenario] if args.scenario != "all" else ["tiny", "small", "medium"]
    )

    run_tests(scenarios, args.tests, args.clean)


if __name__ == "__main__":
    main()
