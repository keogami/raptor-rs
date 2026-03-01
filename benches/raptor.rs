use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use raptor::Timetable;
use raptor::simple::SimpleTimetable;

type Tt = SimpleTimetable<usize, usize, usize>;

/// Single route with `stops` stops and `trips` trips.
fn build_linear(stops: usize, trips: usize) -> Tt {
    let stop_ids: Vec<usize> = (0..stops).collect();
    let trip_defs: Vec<(usize, Vec<(usize, usize)>)> = (0..trips)
        .map(|t| {
            let times: Vec<(usize, usize)> = (0..stops)
                .map(|s| {
                    let base = t * stops * 2;
                    (base + s * 2, base + s * 2 + 1)
                })
                .collect();
            (t, times)
        })
        .collect();

    let mut tt = SimpleTimetable::new();
    let trip_refs: Vec<(usize, &[(usize, usize)])> = trip_defs
        .iter()
        .map(|(id, times)| (*id, times.as_slice()))
        .collect();
    tt = tt.route(0, &stop_ids, &trip_refs);
    tt
}

/// `routes` horizontal routes, each with `stops_per_route` stops, plus
/// `connectors` vertical connector routes linking corresponding stops.
fn build_grid(routes: usize, stops_per_route: usize, connectors: usize) -> Tt {
    let mut tt = SimpleTimetable::new();
    let mut route_id = 0usize;
    let mut trip_id = 0usize;

    // Horizontal routes: route r has stops [r*stops_per_route .. (r+1)*stops_per_route)
    for r in 0..routes {
        let stop_ids: Vec<usize> = (0..stops_per_route)
            .map(|s| r * stops_per_route + s)
            .collect();
        let times: Vec<(usize, usize)> =
            (0..stops_per_route).map(|s| (s * 10, s * 10 + 5)).collect();
        tt = tt.route(route_id, &stop_ids, &[(trip_id, times.as_slice())]);
        route_id += 1;
        trip_id += 1;
    }

    // Vertical connector routes: connect stop `col` across all horizontal routes
    let connector_step = stops_per_route.max(1) / connectors.max(1);
    for c in 0..connectors {
        let col = c * connector_step;
        let stop_ids: Vec<usize> = (0..routes).map(|r| r * stops_per_route + col).collect();
        let times: Vec<(usize, usize)> = (0..routes)
            .map(|r| {
                let base = col * 10 + r * 3;
                (base, base + 1)
            })
            .collect();
        tt = tt.route(route_id, &stop_ids, &[(trip_id, times.as_slice())]);
        route_id += 1;
        trip_id += 1;
    }

    tt
}

/// `hubs` central hub stops, each served by `routes_per_hub` routes radiating out
/// to `spokes` spoke stops. Hubs are connected by footpaths.
fn build_hub_spoke(hubs: usize, routes_per_hub: usize, spokes: usize) -> Tt {
    let mut tt = SimpleTimetable::new();
    let mut route_id = 0usize;
    let mut trip_id = 0usize;
    let mut stop_id = 0usize;

    // Hub stops: 0..hubs
    let hub_stops: Vec<usize> = (0..hubs)
        .map(|_| {
            let s = stop_id;
            stop_id += 1;
            s
        })
        .collect();

    for (h, &hub) in hub_stops.iter().enumerate() {
        for r in 0..routes_per_hub {
            let mut stops = vec![hub];
            for _ in 0..spokes {
                stops.push(stop_id);
                stop_id += 1;
            }
            let times: Vec<(usize, usize)> = stops
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let base = (h * routes_per_hub + r) * (spokes + 1) * 10;
                    (base + i * 10, base + i * 10 + 5)
                })
                .collect();
            tt = tt.route(route_id, &stops, &[(trip_id, times.as_slice())]);
            route_id += 1;
            trip_id += 1;
        }
    }

    // Connect hubs via footpaths
    for i in 0..hubs {
        for j in 0..hubs {
            if i != j {
                tt = tt.footpath(hub_stops[i], hub_stops[j]);
                tt = tt.transfer_time(hub_stops[i], hub_stops[j], 2);
            }
        }
    }

    tt
}

/// N sequential routes forming a chain: route 0 goes A0→A1, route 1 goes A1→A2, etc.
/// Forces `segments` transfers to get from A0 to A_{segments}.
fn build_chain(segments: usize) -> Tt {
    let mut tt = SimpleTimetable::new();

    for seg in 0..segments {
        let from = seg;
        let to = seg + 1;
        let base_time = seg * 20;
        tt = tt.route(
            seg,
            &[from, to],
            &[(
                seg,
                &[(base_time, base_time + 5), (base_time + 10, base_time + 15)],
            )],
        );
    }

    tt
}

/// `path_count` parallel paths from stop 0 to a common target stop.
/// Path i has (i+1) legs, but each successive path is slightly faster overall.
fn build_parallel_paths(path_count: usize, max_legs: usize) -> Tt {
    let mut tt = SimpleTimetable::new();
    let mut route_id = 0usize;
    let mut trip_id = 0usize;
    // Stop 0 = source, stop 1 = target
    // Intermediate stops start from 2
    let mut next_stop = 2usize;

    let source = 0usize;
    let target = 1usize;

    for p in 0..path_count {
        let legs = ((p % max_legs) + 1).min(max_legs);
        // Total journey time decreases with more legs (faster paths have more transfers)
        let total_time = 1000 - p * 50;
        let leg_time = total_time / legs;

        let mut prev_stop = source;
        for leg in 0..legs {
            let is_last = leg == legs - 1;
            let curr_stop = if is_last { target } else { next_stop };
            if !is_last {
                next_stop += 1;
            }

            let depart = leg * leg_time;
            let arrive = depart + leg_time - 5;

            tt = tt.route(
                route_id,
                &[prev_stop, curr_stop],
                &[(trip_id, &[(depart, depart + 1), (arrive, arrive + 1)])],
            );
            route_id += 1;
            trip_id += 1;
            prev_stop = curr_stop;
        }
    }

    tt
}

fn bench_linear(c: &mut Criterion) {
    let mut group = c.benchmark_group("linear");
    for &(stops, trips) in &[(10, 5), (50, 10), (200, 20)] {
        let tt = build_linear(stops, trips);
        group.bench_with_input(
            BenchmarkId::new("raptor", format!("{stops}s_{trips}t")),
            &tt,
            |b, tt| b.iter(|| tt.raptor(3, 0, 0, stops - 1)),
        );
    }
    group.finish();
}

fn bench_grid(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid");
    for &(routes, spr, connectors) in &[(3, 10, 2), (5, 20, 4), (10, 30, 6)] {
        let tt = build_grid(routes, spr, connectors);
        let source = 0;
        let target = (routes - 1) * spr + spr - 1;
        group.bench_with_input(
            BenchmarkId::new("raptor", format!("{routes}r_{spr}s_{connectors}c")),
            &tt,
            |b, tt| b.iter(|| tt.raptor(5, 0, source, target)),
        );
    }
    group.finish();
}

fn bench_hub_spoke(c: &mut Criterion) {
    let mut group = c.benchmark_group("hub_spoke");
    for &(hubs, rph, spokes) in &[(1, 10, 10), (3, 10, 10), (3, 20, 15)] {
        let tt = build_hub_spoke(hubs, rph, spokes);
        // Route from first spoke of first hub to last spoke of last hub
        let source = hubs; // first non-hub stop
        let last_hub_first_route_last_spoke =
            hubs + (hubs - 1) * rph * spokes + (rph - 1) * spokes + spokes - 1;
        let target = last_hub_first_route_last_spoke.min(hubs + hubs * rph * spokes - 1);
        group.bench_with_input(
            BenchmarkId::new("raptor", format!("{hubs}h_{rph}r_{spokes}sp")),
            &tt,
            |b, tt| b.iter(|| tt.raptor(5, 0, source, target)),
        );
    }
    group.finish();
}

fn bench_transfer_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("transfer_scaling");
    let tt = build_grid(5, 20, 4);
    let source = 0;
    let target = 4 * 20 + 19;
    for k in [1, 3, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("raptor", format!("k{k}")), &k, |b, &k| {
            b.iter(|| tt.raptor(k, 0, source, target))
        });
    }
    group.finish();
}

fn bench_reconstruction_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("reconstruction_depth");
    for segments in [3, 5, 10, 20] {
        let tt = build_chain(segments);
        group.bench_with_input(
            BenchmarkId::new("raptor", format!("{segments}seg")),
            &tt,
            |b, tt| b.iter(|| tt.raptor(segments + 1, 0, 0, segments)),
        );
    }
    group.finish();
}

fn bench_reconstruction_breadth(c: &mut Criterion) {
    let mut group = c.benchmark_group("reconstruction_breadth");
    for &(paths, max_legs) in &[(3, 3), (5, 5), (10, 10)] {
        let tt = build_parallel_paths(paths, max_legs);
        group.bench_with_input(
            BenchmarkId::new("raptor", format!("{paths}p_{max_legs}l")),
            &tt,
            |b, tt| b.iter(|| tt.raptor(max_legs + 1, 0, 0, 1)),
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_linear,
    bench_grid,
    bench_hub_spoke,
    bench_transfer_scaling,
    bench_reconstruction_depth,
    bench_reconstruction_breadth,
);
criterion_main!(benches);
