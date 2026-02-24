use std::collections::BTreeMap;

use crate::{Tau, Timetable};

pub struct TestTimetable {
    /// route_id -> ordered stop sequence
    routes: BTreeMap<usize, Vec<usize>>,
    /// trip_id -> (route_id, per-stop (arrival, departure) aligned with route's stop order)
    trips: BTreeMap<usize, (usize, Vec<(Tau, Tau)>)>,
    /// stop -> reachable stops via footpath
    footpaths: BTreeMap<usize, Vec<usize>>,
    /// (from, to) -> transfer time
    transfer_times: BTreeMap<(usize, usize), Tau>,
}

impl TestTimetable {
    fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            trips: BTreeMap::new(),
            footpaths: BTreeMap::new(),
            transfer_times: BTreeMap::new(),
        }
    }

    fn route(mut self, id: usize, stops: &[usize], trips: &[(usize, &[(Tau, Tau)])]) -> Self {
        self.routes.insert(id, stops.to_vec());
        for &(trip_id, times) in trips {
            assert_eq!(
                times.len(),
                stops.len(),
                "trip {trip_id} has {} times but route {id} has {} stops",
                times.len(),
                stops.len()
            );
            self.trips.insert(trip_id, (id, times.to_vec()));
        }
        self
    }

    fn footpath(mut self, from: usize, to: usize) -> Self {
        self.footpaths.entry(from).or_default().push(to);
        self
    }

    fn transfer_time(mut self, from: usize, to: usize, time: Tau) -> Self {
        self.transfer_times.insert((from, to), time);
        self
    }
}

impl From<(&[(usize, &[usize], &[(usize, &[(Tau, Tau)])])], &[(usize, usize)])>
    for TestTimetable
{
    fn from(
        (route_defs, footpath_defs): (
            &[(usize, &[usize], &[(usize, &[(Tau, Tau)])])],
            &[(usize, usize)],
        ),
    ) -> Self {
        let mut tt = Self::new();
        for &(route_id, stops, trips) in route_defs {
            tt.routes.insert(route_id, stops.to_vec());
            for &(trip_id, times) in trips {
                assert_eq!(
                    times.len(),
                    stops.len(),
                    "trip {trip_id} has {} times but route {route_id} has {} stops",
                    times.len(),
                    stops.len()
                );
                tt.trips.insert(trip_id, (route_id, times.to_vec()));
            }
        }
        for &(from, to) in footpath_defs {
            tt.footpaths.entry(from).or_default().push(to);
        }
        tt
    }
}

impl Timetable for TestTimetable {
    type Stop = usize;
    type Route = usize;
    type Trip = usize;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        self.routes
            .iter()
            .filter(|(_, stops)| stops.contains(&stop))
            .map(|(&route_id, _)| route_id)
            .collect()
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let stops = &self.routes[&route];
        let l = stops.iter().position(|&s| s == left).unwrap();
        let r = stops.iter().position(|&s| s == right).unwrap();
        stops[l.min(r)]
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        let stops = &self.routes[&route];
        let pos = stops.iter().position(|&s| s == stop).unwrap();
        stops[pos..].to_vec()
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let stops = &self.routes[&route];
        let stop_idx = stops.iter().position(|&s| s == stop)?;

        self.trips
            .iter()
            .filter(|(_, (r, _))| *r == route)
            .filter(|(_, (_, times))| times[stop_idx].1 >= at)
            .min_by_key(|(_, (_, times))| times[stop_idx].1)
            .map(|(&trip_id, _)| trip_id)
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        let (route_id, times) = &self.trips[&trip];
        let stops = &self.routes[route_id];
        let idx = stops.iter().position(|&s| s == stop).unwrap();
        times[idx].0
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        let (route_id, times) = &self.trips[&trip];
        let stops = &self.routes[route_id];
        let idx = stops.iter().position(|&s| s == stop).unwrap();
        times[idx].1
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Vec<Self::Stop> {
        self.footpaths.get(&stop).cloned().unwrap_or_default()
    }

    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> Tau {
        self.transfer_times.get(&(from, to)).copied().unwrap_or(1)
    }
}

/// Recreates the Issue1 network from examples/reconstruction_bugs.rs
///
/// Stop mapping: S=0, A=1, B=2, C=3, D=4
/// Route mapping: R1=0, R2=1, R3=2
/// Trip mapping: R1's trip=10, R2's trip=20, R3/T1(late)=31, R3/T2(early)=32
#[test]
fn reconstruction_bugs_issue1() {
    let tt = TestTimetable::new()
        .route(0, &[0, 1], &[(10, &[(0, 0), (100, 100)])])
        .route(1, &[0, 2], &[(20, &[(0, 0), (30, 30)])])
        .route(
            2,
            &[1, 2, 3, 4],
            &[
                (31, &[(105, 105), (110, 110), (120, 120), (130, 130)]),
                (32, &[(25, 25), (30, 30), (40, 40), (50, 50)]),
            ],
        );

    let journeys = tt.raptor(3, 0, 0, 4);

    // The optimal journey: S→B via R2, then B→D via R3/T2, arriving at t=50
    // Plan: [(R2, S), (R3, B)] = [(1, 0), (2, 2)]
    assert!(!journeys.is_empty(), "should find at least one journey");

    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 50);
    assert_eq!(best.plan, vec![(1, 0), (2, 2)]);
}

/// Same test using the From impl for static declaration
#[test]
fn reconstruction_bugs_issue1_from() {
    let routes: &[(usize, &[usize], &[(usize, &[(Tau, Tau)])])] = &[
        (0, &[0, 1], &[(10, &[(0, 0), (100, 100)])]),
        (1, &[0, 2], &[(20, &[(0, 0), (30, 30)])]),
        (
            2,
            &[1, 2, 3, 4],
            &[
                (31, &[(105, 105), (110, 110), (120, 120), (130, 130)]),
                (32, &[(25, 25), (30, 30), (40, 40), (50, 50)]),
            ],
        ),
    ];
    let tt = TestTimetable::from((routes, &[] as &[_]));

    let journeys = tt.raptor(3, 0, 0, 4);

    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 50);
    assert_eq!(best.plan, vec![(1, 0), (2, 2)]);
}
