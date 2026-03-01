use criterion::{Criterion, criterion_group, criterion_main};
use gtfs_structures::Gtfs;
use raptor::Timetable;
use raptor::gtfs::GtfsTimetable;
use std::hint::black_box;

const GTFS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../aux/dmrc_gtfs.zip");

fn bench_gtfs_load(c: &mut Criterion) {
    c.bench_function("gtfs_load", |b| {
        b.iter(|| {
            let gtfs = Gtfs::new(GTFS_PATH).unwrap();
            let timetable = GtfsTimetable::new(&gtfs).unwrap();
            black_box(&timetable);
        });
    });
}

criterion_group!(gtfs_benches, bench_gtfs_load);
criterion_main!(gtfs_benches);
