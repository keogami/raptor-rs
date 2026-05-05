use super::SimpleTimetable;

type Tt = SimpleTimetable<usize, usize, usize>;

/// Single route with `stops` stops and `trips` trips.
pub fn build_linear(stops: usize, trips: usize) -> Tt {
    let stop_ids: Vec<usize> = (0..stops).collect();
    let trip_defs: Vec<(usize, Vec<(crate::SecondOfDay, crate::SecondOfDay)>)> = (0..trips)
        .map(|t| {
            let times: Vec<(crate::SecondOfDay, crate::SecondOfDay)> = (0..stops)
                .map(|s| {
                    let base = t * stops * 2;
                    (
                        crate::SecondOfDay((base + s * 2) as u32),
                        crate::SecondOfDay((base + s * 2 + 1) as u32),
                    )
                })
                .collect();
            (t, times)
        })
        .collect();

    let mut tt = SimpleTimetable::new();
    let trip_refs: Vec<(usize, &[(crate::SecondOfDay, crate::SecondOfDay)])> = trip_defs
        .iter()
        .map(|(id, times)| (*id, times.as_slice()))
        .collect();
    tt = tt.route(0, &stop_ids, &trip_refs);
    tt
}

/// `routes` horizontal routes, each with `stops_per_route` stops, plus
/// `connectors` vertical connector routes linking corresponding stops.
pub fn build_grid(routes: usize, stops_per_route: usize, connectors: usize) -> Tt {
    let mut tt = SimpleTimetable::new();
    let mut route_id = 0usize;
    let mut trip_id = 0usize;

    // Horizontal routes: route r has stops [r*stops_per_route .. (r+1)*stops_per_route)
    for r in 0..routes {
        let stop_ids: Vec<usize> = (0..stops_per_route)
            .map(|s| r * stops_per_route + s)
            .collect();
        let times: Vec<(crate::SecondOfDay, crate::SecondOfDay)> = (0..stops_per_route)
            .map(|s| {
                (
                    crate::SecondOfDay((s * 10) as u32),
                    crate::SecondOfDay((s * 10 + 5) as u32),
                )
            })
            .collect();
        tt = tt.route(route_id, &stop_ids, &[(trip_id, times.as_slice())]);
        route_id += 1;
        trip_id += 1;
    }

    // Vertical connector routes: connect stop `col` across all horizontal routes
    let connector_step = stops_per_route.max(1) / connectors.max(1);
    for c in 0..connectors {
        let col = c * connector_step;
        let stop_ids: Vec<usize> = (0..routes).map(|r| r * stops_per_route + col).collect();
        let times: Vec<(crate::SecondOfDay, crate::SecondOfDay)> = (0..routes)
            .map(|r| {
                let base = col * 10 + r * 3;
                (
                    crate::SecondOfDay((base) as u32),
                    crate::SecondOfDay((base + 1) as u32),
                )
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
pub fn build_hub_spoke(hubs: usize, routes_per_hub: usize, spokes: usize) -> Tt {
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
            let times: Vec<(crate::SecondOfDay, crate::SecondOfDay)> = stops
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let base = (h * routes_per_hub + r) * (spokes + 1) * 10;
                    (
                        crate::SecondOfDay((base + i * 10) as u32),
                        crate::SecondOfDay((base + i * 10 + 5) as u32),
                    )
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
                tt = tt.transfer_time(hub_stops[i], hub_stops[j], crate::Duration(2));
            }
        }
    }

    tt
}

/// N sequential routes forming a chain: route 0 goes A0→A1, route 1 goes A1→A2, etc.
/// Forces `segments` transfers to get from A0 to A_{segments}.
pub fn build_chain(segments: usize) -> Tt {
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
                &[
                    (
                        crate::SecondOfDay(base_time as u32),
                        crate::SecondOfDay((base_time + 5) as u32),
                    ),
                    (
                        crate::SecondOfDay((base_time + 10) as u32),
                        crate::SecondOfDay((base_time + 15) as u32),
                    ),
                ],
            )],
        );
    }

    tt
}

/// `path_count` parallel paths from stop 0 to a common target stop.
/// Path i has (i+1) legs, but each successive path is slightly faster overall.
pub fn build_parallel_paths(path_count: usize, max_legs: usize) -> Tt {
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
                &[(
                    trip_id,
                    &[
                        (
                            crate::SecondOfDay(depart as u32),
                            crate::SecondOfDay((depart + 1) as u32),
                        ),
                        (
                            crate::SecondOfDay(arrive as u32),
                            crate::SecondOfDay((arrive + 1) as u32),
                        ),
                    ],
                )],
            );
            route_id += 1;
            trip_id += 1;
            prev_stop = curr_stop;
        }
    }

    tt
}
