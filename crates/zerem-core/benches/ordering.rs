//! What the table costs per tick, measured rather than assumed.
//!
//! Three things run over every row, once a second, while the window is open:
//! the sort, the filter, and the rate smoothing. Nothing else in the app is
//! shaped like that — everything else is per click or per torrent — so these
//! are the three that turn a big list into a slow one.
//!
//! Run with `cargo bench -p zerem-core`. The numbers are for whoever is
//! changing this code; what guards the app in CI is the budget test in
//! `sort.rs`, which is deliberately loose and catches the only regression that
//! actually matters — somebody making one of these quadratic.

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use zerem_core::{Filter, Rate, Sort, TorrentId, TorrentRow};

/// A list shaped like a real one: names that share long prefixes, so the
/// comparator has to read past them, and sizes that do not arrive in order.
fn rows(count: usize) -> Vec<TorrentRow> {
    (0..count)
        .map(|i| {
            let name = format!("Some.Release.Name.S{:02}E{:02}.1080p.WEB-DL.x265-GROUP", i % 40, i % 24);
            let key: Arc<str> = Arc::from(name.to_lowercase().as_str());
            let mut row = TorrentRow::shared(
                TorrentId(i as u32),
                Arc::from(name.as_str()),
                key,
                // Spread, and not monotonic with the index: a size sort that
                // got a sorted input would measure the best case only.
                ((i * 7919) % 100_000) as u64 * 1024 * 1024,
            );
            row.done = row.size / 2;
            row
        })
        .collect()
}

fn sorting(c: &mut Criterion) {
    let mut group = c.benchmark_group("sort");
    for count in [200usize, 2_000, 20_000] {
        let rows = rows(count);
        let mut out = Vec::with_capacity(count);
        // By name, which is the expensive column: every other one compares two
        // numbers, and this one walks two strings that agree for thirty bytes.
        group.bench_with_input(BenchmarkId::new("by-name", count), &rows, |b, rows| {
            b.iter(|| zerem_core::sort::order(black_box(rows), Sort::default(), &mut out));
        });
    }
    group.finish();
}

fn filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter");
    let rows = rows(20_000);
    // Three words, none of which is the first thing in the name: a filter that
    // matches on the first byte measures the rejection path and nothing else.
    let filter = Filter::new("web x265 group");
    group.bench_function("three-words-over-20k", |b| {
        b.iter(|| black_box(&rows).iter().filter(|row| filter.matches(row)).count());
    });
    group.finish();
}

fn smoothing(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate");
    // One per torrent per tick, so twenty thousand of them is one tick of a
    // very large session.
    group.bench_function("update-20k", |b| {
        b.iter_batched(
            || vec![Rate::default(); 20_000],
            |mut rates| {
                for (i, rate) in rates.iter_mut().enumerate() {
                    black_box(rate.update((i as u64) * 1024));
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(benches, sorting, filtering, smoothing);
criterion_main!(benches);
