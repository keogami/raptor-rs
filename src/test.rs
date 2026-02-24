use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::{Tau, Timetable};

pub struct TestTimetable<S, R, T> {
    /// route_id -> ordered stop sequence
    routes: BTreeMap<R, Vec<S>>,
    /// trip_id -> (route_id, per-stop (arrival, departure) aligned with route's stop order)
    trips: BTreeMap<T, (R, Vec<(Tau, Tau)>)>,
    /// stop -> reachable stops via footpath
    footpaths: BTreeMap<S, Vec<S>>,
    /// (from, to) -> transfer time
    transfer_times: BTreeMap<(S, S), Tau>,
}

impl<S, R, T> TestTimetable<S, R, T>
where
    S: Ord + Copy,
    R: Ord + Copy,
    T: Ord + Copy,
{
    fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            trips: BTreeMap::new(),
            footpaths: BTreeMap::new(),
            transfer_times: BTreeMap::new(),
        }
    }

    fn route(mut self, id: R, stops: &[S], trips: &[(T, &[(Tau, Tau)])]) -> Self
    where
        R: Debug,
        T: Debug,
    {
        self.routes.insert(id, stops.to_vec());
        for &(trip_id, times) in trips {
            assert_eq!(
                times.len(),
                stops.len(),
                "trip {trip_id:?} has {} times but route {id:?} has {} stops",
                times.len(),
                stops.len()
            );
            self.trips.insert(trip_id, (id, times.to_vec()));
        }
        self
    }

    fn footpath(mut self, from: S, to: S) -> Self {
        self.footpaths.entry(from).or_default().push(to);
        self
    }

    fn transfer_time(mut self, from: S, to: S, time: Tau) -> Self {
        self.transfer_times.insert((from, to), time);
        self
    }
}

impl<S, R, T> From<(&[(R, &[S], &[(T, &[(Tau, Tau)])])], &[(S, S)])>
    for TestTimetable<S, R, T>
where
    S: Ord + Copy,
    R: Ord + Copy + Debug,
    T: Ord + Copy + Debug,
{
    fn from(
        (route_defs, footpath_defs): (
            &[(R, &[S], &[(T, &[(Tau, Tau)])])],
            &[(S, S)],
        ),
    ) -> Self {
        let mut tt = Self::new();
        for &(route_id, stops, trips) in route_defs {
            tt.routes.insert(route_id, stops.to_vec());
            for &(trip_id, times) in trips {
                assert_eq!(
                    times.len(),
                    stops.len(),
                    "trip {trip_id:?} has {} times but route {route_id:?} has {} stops",
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

impl<S, R, T> Timetable for TestTimetable<S, R, T>
where
    S: Ord + Copy + Debug,
    R: Ord + Copy + Debug,
    T: Ord + Copy + Debug,
{
    type Stop = S;
    type Route = R;
    type Trip = T;

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
/// Stop mapping: S, A, B, C, D
/// Route mapping: R1, R2, R3
/// Trip mapping: R1T1, R2T1, R3Late, R3Early
#[test]
fn reconstruction_bugs_issue1() {
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop { S, A, B, C, D }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route { R1, R2, R3 }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip { R1T1, R2T1, R3Late, R3Early }

    let tt = TestTimetable::new()
        .route(R1, &[S, A], &[(R1T1, &[(0, 0), (100, 100)])])
        .route(R2, &[S, B], &[(R2T1, &[(0, 0), (30, 30)])])
        .route(
            R3,
            &[A, B, C, D],
            &[
                (R3Late, &[(105, 105), (110, 110), (120, 120), (130, 130)]),
                (R3Early, &[(25, 25), (30, 30), (40, 40), (50, 50)]),
            ],
        );

    let journeys = tt.raptor(3, 0, S, D);

    // The optimal journey: S->B via R2, then B->D via R3/Early, arriving at t=50
    assert!(!journeys.is_empty(), "should find at least one journey");

    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 50);
    assert_eq!(best.plan, vec![(R2, S), (R3, B)]);
}

/// Same test using the From impl for static declaration
#[test]
fn reconstruction_bugs_issue1_from() {
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop { S, A, B, C, D }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route { R1, R2, R3 }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip { R1T1, R2T1, R3Late, R3Early }

    let routes: &[(Route, &[Stop], &[(Trip, &[(Tau, Tau)])])] = &[
        (R1, &[S, A], &[(R1T1, &[(0, 0), (100, 100)])]),
        (R2, &[S, B], &[(R2T1, &[(0, 0), (30, 30)])]),
        (
            R3,
            &[A, B, C, D],
            &[
                (R3Late, &[(105, 105), (110, 110), (120, 120), (130, 130)]),
                (R3Early, &[(25, 25), (30, 30), (40, 40), (50, 50)]),
            ],
        ),
    ];
    let tt = TestTimetable::from((routes, &[] as &[_]));

    let journeys = tt.raptor(3, 0, S, D);

    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 50);
    assert_eq!(best.plan, vec![(R2, S), (R3, B)]);
}
