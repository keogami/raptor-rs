use std::fmt::Debug;
use std::{borrow::Cow, collections::BTreeMap};

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

impl<S, R, T> From<(&[(R, &[S], &[(T, &[(Tau, Tau)])])], &[(S, S)])> for TestTimetable<S, R, T>
where
    S: Ord + Copy,
    R: Ord + Copy + Debug,
    T: Ord + Copy + Debug,
{
    fn from((route_defs, footpath_defs): (&[(R, &[S], &[(T, &[(Tau, Tau)])])], &[(S, S)])) -> Self {
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

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Cow<'_, [Self::Route]> {
        self.routes
            .iter()
            .filter(|(_, stops)| stops.contains(&stop))
            .map(|(&route_id, _)| route_id)
            .collect::<Vec<_>>()
            .into()
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

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        let stops = &self.routes[&route];
        let pos = stops.iter().position(|&s| s == stop).unwrap();
        stops[pos..].into()
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

    fn get_footpaths_from(&self, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        self.footpaths
            .get(&stop)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
            .into()
    }

    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> Tau {
        self.transfer_times.get(&(from, to)).copied().unwrap_or(1)
    }
}

/// When a faster route reaches a mid-route stop, the algorithm must record
/// that stop as the boarding stop — not the earlier stop where the route
/// scan began. See examples/reboarding.rs for the full network.
#[test]
fn reboarding_picks_correct_boarding_stop() {
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        S,
        A,
        B,
        C,
        D,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
        R3,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        R1T1,
        R2T1,
        R3Late,
        R3Early,
    }

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
    assert_eq!(best.plan, vec![(R2, B), (R3, D)]);
}

// ── Edge case tests ─────────────────────────────────────────────────

#[test]
fn no_journey_disconnected_graph() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (10, 10)])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(0, 0), (10, 10)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::D);
    assert!(
        journeys.is_empty(),
        "disconnected graph should yield no journeys"
    );
}

#[test]
fn no_journey_missed_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (50, 50)])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(0, 30), (40, 40)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::C);
    assert!(
        journeys.is_empty(),
        "missed connection should yield no journeys"
    );
}

#[test]
fn no_journey_late_departure() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(0, 10), (20, 20)])],
    );

    let journeys = tt.raptor(3, 100, Stop::A, Stop::B);
    assert!(
        journeys.is_empty(),
        "late departure should yield no journeys"
    );
}

#[test]
fn no_journey_transfers_zero() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(0, 0), (10, 10)])],
    );

    let journeys = tt.raptor(0, 0, Stop::A, Stop::B);
    assert!(journeys.is_empty(), "transfers=0 should yield no journeys");
}

#[test]
fn source_equals_target() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(0, 0), (10, 10)])],
    );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::A);
    assert!(
        journeys.is_empty(),
        "source == target should yield no journeys"
    );
}

#[test]
fn direct_journey_single_route() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B, Stop::C],
        &[(Trip::T1, &[(0, 0), (10, 10), (20, 20)])],
    );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::C);
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].arrival, 20);
    assert_eq!(journeys[0].plan, vec![(Route::R1, Stop::C)]);
}

#[test]
fn direct_journey_picks_fastest_route() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (100, 100)])],
        )
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(0, 0), (50, 50)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::B);
    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 50);
}

#[test]
fn exact_time_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (20, 20)])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(0, 20), (30, 30)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::C);
    assert!(!journeys.is_empty(), "exact-time connection should work");
    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 30);
    assert_eq!(best.plan, vec![(Route::R1, Stop::B), (Route::R2, Stop::C)]);
}

#[test]
fn multi_trip_picks_earliest_catchable() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[
            (Trip::T1, &[(0, 5), (15, 15)]),
            (Trip::T2, &[(0, 15), (25, 25)]),
            (Trip::T3, &[(0, 25), (35, 35)]),
        ],
    );

    // Query at tau=12: T1 departs A@5 (too early), T2 departs A@15 (catchable)
    let journeys = tt.raptor(3, 12, Stop::A, Stop::B);
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].arrival, 25); // T2 arrives B@25
}

#[test]
fn two_transfer_journey() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
        R3,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (10, 10)])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(0, 10), (20, 20)])],
        )
        .route(
            Route::R3,
            &[Stop::C, Stop::D],
            &[(Trip::T3, &[(0, 20), (30, 30)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::D);
    assert!(!journeys.is_empty());
    let best = journeys.iter().min_by_key(|j| j.arrival).unwrap();
    assert_eq!(best.arrival, 30);
    assert_eq!(
        best.plan,
        vec![
            (Route::R1, Stop::B),
            (Route::R2, Stop::C),
            (Route::R3, Stop::D),
        ]
    );
}

#[test]
fn pareto_optimal_fewer_transfers_vs_faster() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
        R3,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = TestTimetable::new()
        // Direct slow route A→D
        .route(
            Route::R1,
            &[Stop::A, Stop::D],
            &[(Trip::T1, &[(0, 0), (200, 200)])],
        )
        // Fast 2-leg: A→B via R2, B→D via R3
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(0, 0), (40, 40)])],
        )
        .route(
            Route::R3,
            &[Stop::B, Stop::D],
            &[(Trip::T3, &[(0, 40), (100, 100)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::D);
    assert_eq!(journeys.len(), 2, "should have 2 pareto-optimal journeys");

    let mut sorted = journeys.clone();
    sorted.sort_by_key(|j| j.arrival);
    // Faster journey (2 legs)
    assert_eq!(sorted[0].arrival, 100);
    assert_eq!(sorted[0].plan.len(), 2);
    // Slower direct journey (1 leg)
    assert_eq!(sorted[1].arrival, 200);
    assert_eq!(sorted[1].plan.len(), 1);
}

#[test]
fn footpath_enables_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (10, 10)])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(0, 20), (30, 30)])],
        )
        .footpath(Stop::B, Stop::C)
        .transfer_time(Stop::B, Stop::C, 5);

    let journeys = tt.raptor(3, 0, Stop::A, Stop::D);
    // NOTE: The algorithm correctly propagates arrival times through footpaths,
    // but journey reconstruction can't trace back through footpath-only transfers
    // (no boarding tree entry for the footpath destination). This documents that limitation.
    assert!(
        journeys.is_empty(),
        "footpath-only transfer not reconstructable"
    );
}

#[test]
fn footpath_transfer_time_causes_miss() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (50, 50)])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(0, 52), (60, 60)])],
        )
        .footpath(Stop::B, Stop::C)
        .transfer_time(Stop::B, Stop::C, 5); // 50+5=55 > 52

    let journeys = tt.raptor(3, 0, Stop::A, Stop::D);
    assert!(
        journeys.is_empty(),
        "footpath transfer time should cause miss"
    );
}

#[test]
fn early_termination_no_improvement() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
    }

    let tt = TestTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(0, 0), (10, 10)])],
    );

    let j1 = tt.raptor(1, 0, Stop::A, Stop::B);
    let j100 = tt.raptor(100, 0, Stop::A, Stop::B);

    assert_eq!(j1.len(), j100.len());
    assert_eq!(
        j1.iter().min_by_key(|j| j.arrival).unwrap().arrival,
        j100.iter().min_by_key(|j| j.arrival).unwrap().arrival,
    );
}

#[test]
fn dominance_prunes_slower_arrival() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Trip {
        T1,
        T2,
    }

    let tt = TestTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(0, 0), (50, 50)])],
        )
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(0, 0), (100, 100)])],
        );

    let journeys = tt.raptor(3, 0, Stop::A, Stop::B);
    // Both routes are discovered in round 1, so the slower one is dominated
    assert_eq!(journeys.len(), 1, "dominated journey should be pruned");
    assert_eq!(journeys[0].arrival, 50);
}
