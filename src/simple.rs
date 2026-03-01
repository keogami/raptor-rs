use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::{Tau, Timetable};

pub struct SimpleTimetable<S, R, T> {
    /// route_id -> ordered stop sequence
    routes: BTreeMap<R, Vec<S>>,
    /// trip_id -> (route_id, per-stop (arrival, departure) aligned with route's stop order)
    trips: BTreeMap<T, (R, Vec<(Tau, Tau)>)>,
    /// stop -> reachable stops via footpath
    footpaths: BTreeMap<S, Vec<S>>,
    /// (from, to) -> transfer time
    transfer_times: BTreeMap<(S, S), Tau>,
}

impl<S, R, T> SimpleTimetable<S, R, T>
where
    S: Ord + Copy,
    R: Ord + Copy,
    T: Ord + Copy,
{
    pub fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            trips: BTreeMap::new(),
            footpaths: BTreeMap::new(),
            transfer_times: BTreeMap::new(),
        }
    }

    pub fn route(mut self, id: R, stops: &[S], trips: &[(T, &[(Tau, Tau)])]) -> Self
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

    pub fn footpath(mut self, from: S, to: S) -> Self {
        self.footpaths.entry(from).or_default().push(to);
        self
    }

    pub fn transfer_time(mut self, from: S, to: S, time: Tau) -> Self {
        self.transfer_times.insert((from, to), time);
        self
    }
}

impl<S, R, T> From<(&[(R, &[S], &[(T, &[(Tau, Tau)])])], &[(S, S)])> for SimpleTimetable<S, R, T>
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

impl<S, R, T> Timetable for SimpleTimetable<S, R, T>
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
