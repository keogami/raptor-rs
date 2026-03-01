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

fn bench_gtfs_all_pairs(c: &mut Criterion) {
    let gtfs = Gtfs::new(GTFS_PATH).unwrap();
    let timetable = GtfsTimetable::new(&gtfs).unwrap();
    let stops: Vec<&str> = gtfs.stops.keys().map(|s| s.as_str()).collect();

    c.bench_function("gtfs_all_pairs", |b| {
        b.iter(|| {
            for &source in &stops {
                for &target in &stops {
                    if source == target {
                        continue;
                    }
                    black_box(timetable.raptor(10, 0, source, target));
                }
            }
        });
    });
}

criterion_group!(gtfs_benches, bench_gtfs_load, bench_gtfs_all_pairs);
criterion_main!(gtfs_benches);
