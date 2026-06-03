//! Heap profiling guidance for hot codec paths.
//!
//! The `dhat` global allocator cannot be combined with irosh's dependency tree,
//! which already installs a global allocator. Use one of these alternatives:
//!
//! ## Option 1: Criterion allocation profiling (nightly)
//!
//! ```ignore
//! // in benches/benchmarks.rs, add:
//! use criterion::BenchmarkId;
//!
//! fn bench(c: &mut Criterion) {
//!     let mut group = c.benchmark_group("frame-codec");
//!     group.throughput(Throughput::Bytes(65536));
//!     // With nightly: group.profiler(CriterionProfiler::new());
//!     group.bench_function("round-trip-get-request", |b| {
//!         b.iter(|| black_box(round_trip_get_request()))
//!     });
//!     group.finish();
//! }
//! ```
//!
//! ## Option 2: `dhat` with a separate binary (outside the workspace)
//!
//! Create a new crate that depends on `irosh` and `dhat`, then run with
//! `RUSTFLAGS='--cfg dhat'` or feature-flag the global allocator.
//!
//! ## Option 3: Linux `perf` + `heaptrack`
//!
//! ```bash
//! heaptrack cargo bench --bench benchmarks
//! heaptrack_gui heaptrack.irosh.*.zst
//! ```

fn main() {
    eprintln!(
        "Heap profiling requires a standalone binary outside the workspace.\n\
         See the comments at the top of this file for alternatives."
    );
}
