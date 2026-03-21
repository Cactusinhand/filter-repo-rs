//! Benchmark for SHA lookup patterns used by StripShaLookup and BlobSizeTracker.
//!
//! StripShaLookup keeps sorted [u8; 20] arrays (in-memory for ≤10K, on-disk
//! beyond). This bench measures the two hot-path operations:
//! 1. In-memory binary search on sorted SHA arrays
//! 2. Disk-based binary search (seeking through a sorted temp file)
//! 3. HashSet<Vec<u8>> lookup (used by BlobSizeTracker.oversize)

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::NamedTempFile;

const SHA_BIN_LEN: usize = 20;
type ShaBytes = [u8; SHA_BIN_LEN];

/// Generate deterministic SHA-like bytes for index `i`.
fn make_sha(i: u64) -> ShaBytes {
    let mut out = [0u8; SHA_BIN_LEN];
    // Use a simple hash-like derivation that produces well-distributed bytes
    let mut v = i.wrapping_mul(0x517cc1b727220a95);
    for chunk in out.chunks_exact_mut(4) {
        v = v.wrapping_mul(0x2545F4914F6CDD1D).wrapping_add(1);
        let bytes = v.to_le_bytes();
        chunk.copy_from_slice(&bytes[..4]);
    }
    out
}

/// Build a sorted, dedup'd vector of `n` SHA entries.
fn make_sorted_shas(n: usize) -> Vec<ShaBytes> {
    let mut entries: Vec<ShaBytes> = (0..n as u64).map(make_sha).collect();
    entries.sort_unstable();
    entries.dedup();
    entries
}

/// Write sorted SHAs to a temp file (replicates TempSortedFile).
fn make_sorted_file(entries: &[ShaBytes]) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    {
        let mut bw = BufWriter::new(f.as_file_mut());
        for entry in entries {
            bw.write_all(entry).unwrap();
        }
        bw.flush().unwrap();
    }
    f.as_file_mut().seek(SeekFrom::Start(0)).unwrap();
    f
}

/// Binary search on disk (replicates TempSortedFile::contains).
fn disk_binary_search(file: &mut File, entries: u64, needle: &ShaBytes) -> bool {
    let mut left: u64 = 0;
    let mut right: u64 = entries;
    let mut buf: ShaBytes = [0u8; SHA_BIN_LEN];
    while left < right {
        let mid = (left + right) / 2;
        file.seek(SeekFrom::Start(mid * SHA_BIN_LEN as u64))
            .unwrap();
        file.read_exact(&mut buf).unwrap();
        match buf.cmp(needle) {
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

// ---------------------------------------------------------------------------
// In-memory binary search
// ---------------------------------------------------------------------------

fn bench_inmemory_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha_lookup_inmemory");

    for &n in &[100usize, 1_000, 10_000] {
        let entries = make_sorted_shas(n);

        // Hit: look up an entry that exists (middle of array)
        let hit_needle = entries[entries.len() / 2];
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| entries.binary_search(black_box(&hit_needle)).is_ok())
        });

        // Miss: look up an entry that doesn't exist
        let miss_needle = make_sha(n as u64 + 999_999);
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| entries.binary_search(black_box(&miss_needle)).is_ok())
        });
    }

    // Batch: 1000 lookups against 10K entries (simulates stream processing)
    let entries = make_sorted_shas(10_000);
    let queries: Vec<ShaBytes> = (0..1000)
        .map(|i| {
            if i % 2 == 0 {
                entries[i * (entries.len() / 1000)]
            } else {
                make_sha(100_000 + i as u64)
            }
        })
        .collect();
    group.bench_function("batch_1000_in_10K", |b| {
        b.iter(|| {
            queries
                .iter()
                .filter(|q| entries.binary_search(black_box(q)).is_ok())
                .count()
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Disk-based binary search
// ---------------------------------------------------------------------------

fn bench_disk_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha_lookup_disk");

    for &n in &[10_000usize, 50_000, 100_000] {
        let entries = make_sorted_shas(n);
        let mut tmpfile = make_sorted_file(&entries);
        let count = entries.len() as u64;

        // Hit
        let hit_needle = entries[entries.len() / 2];
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| disk_binary_search(tmpfile.as_file_mut(), count, black_box(&hit_needle)))
        });

        // Miss
        let miss_needle = make_sha(n as u64 + 999_999);
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| disk_binary_search(tmpfile.as_file_mut(), count, black_box(&miss_needle)))
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// HashSet<Vec<u8>> lookup (BlobSizeTracker.oversize pattern)
// ---------------------------------------------------------------------------

fn bench_hashset_sha_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha_lookup_hashset");

    for &n in &[100usize, 1_000, 10_000, 50_000] {
        let set: HashSet<Vec<u8>> = (0..n as u64).map(|i| make_sha(i).to_vec()).collect();

        // Hit
        let hit = make_sha((n / 2) as u64).to_vec();
        group.bench_with_input(BenchmarkId::new("hit", n), &n, |b, _| {
            b.iter(|| set.contains(black_box(&hit)))
        });

        // Miss
        let miss = make_sha(n as u64 + 999_999).to_vec();
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| set.contains(black_box(&miss)))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_inmemory_lookup,
    bench_disk_lookup,
    bench_hashset_sha_lookup
);
criterion_main!(benches);
