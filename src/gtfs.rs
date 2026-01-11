use std::{collections::BTreeMap, sync::OnceLock};

use gtfs_structures::Gtfs;
use smallvec::SmallVec;

use crate::Timetable;

type Route = usize;
type Trip = usize;
type Stop = usize;

type RoutesForStops = BTreeMap<Stop, SmallVec<[Route; 8]>>;
type StopForRoutes = BTreeMap<Route, SmallVec<[Stop; 32]>>;
type TripsForRoutes = BTreeMap<Route, Vec<Trip>>;

pub struct GtfsTimetable<'gtfs> {
    gtfs: &'gtfs Gtfs,

    stops: Vec<&'gtfs str>,
    trips: Vec<&'gtfs str>,
    routes: Vec<&'gtfs str>,

    // can use docs.rs/arc-swap's cache for realtime support
    routes_for_stops: OnceLock<RoutesForStops>,
    stops_for_routes: OnceLock<StopForRoutes>,
    trips_for_routes: OnceLock<TripsForRoutes>,
}

impl<'a> GtfsTimetable<'a> {
    pub fn new(gtfs: &'a Gtfs) -> Self {
        let mut stops: Vec<_> = gtfs.stops.keys().map(String::as_str).collect();
        stops.sort();
        let mut routes: Vec<_> = gtfs.routes.keys().map(String::as_str).collect();
        routes.sort();
        let mut trips: Vec<_> = gtfs.trips.keys().map(String::as_str).collect();
        trips.sort();

        Self {
            gtfs,
            stops,
            routes,
            trips,
            routes_for_stops: Default::default(),
            stops_for_routes: Default::default(),
            trips_for_routes: Default::default(),
        }
    }

    fn cache_routes_for_stops(&self) -> RoutesForStops {
        let mut routes_for_stops = RoutesForStops::default();

        for trip in self.gtfs.trips.values() {
            let route = self.route_idx(&trip.route_id).unwrap();
            for st in &trip.stop_times {
                let stop = self.stop_idx(&st.stop.id).unwrap();
                routes_for_stops.entry(stop).or_default().push(route);
            }
        }

        routes_for_stops
    }

    fn cache_stops_for_routes(&self) -> StopForRoutes {
        let mut stops_for_routes = StopForRoutes::default();

        for trip in self.gtfs.trips.values() {
            let route = self.route_idx(&trip.route_id).unwrap();
            // TODO: handle case where multiple trips run on a route but with different patterns
            // which require merging stops in a meaningful way

            if stops_for_routes.contains_key(&route) {
                continue;
            }

            for st in &trip.stop_times {
                let stop = self.stop_idx(&st.stop.id).unwrap();
                stops_for_routes.entry(route).or_default().push(stop);
            }
        }

        stops_for_routes
    }

    fn cache_trips_for_routes(&self) -> TripsForRoutes {
        let mut trips_for_routes = TripsForRoutes::default();

        for (trip_id, trip) in &self.gtfs.trips {
            let route = self.route_idx(&trip.route_id).unwrap();
            let trip_idx = self.trip_idx(trip_id).unwrap();
            trips_for_routes.entry(route).or_default().push(trip_idx);
        }

        // Sort each route's trips by first stop departure time
        for trips in trips_for_routes.values_mut() {
            trips.sort_by_key(|&trip_idx| {
                let trip_id = &self.trips[trip_idx];
                let trip = self.gtfs.get_trip(trip_id).unwrap();
                trip.stop_times
                    .first()
                    .and_then(|st| st.departure_time)
                    .unwrap_or(u32::MAX)
            });
        }

        trips_for_routes
    }

    fn stop_idx(&self, id: &str) -> Option<usize> {
        self.stops.binary_search_by(|item| (*item).cmp(id)).ok()
    }
    fn trip_idx(&self, id: &str) -> Option<usize> {
        self.trips.binary_search_by(|item| (*item).cmp(id)).ok()
    }
    fn route_idx(&self, id: &str) -> Option<usize> {
        self.routes.binary_search_by(|item| (*item).cmp(id)).ok()
    }

    pub fn resolve_stop(&self, idx: usize) -> Option<&str> {
        self.stops.get(idx).copied()
    }

    pub fn resolve_route(&self, idx: usize) -> Option<&str> {
        self.routes.get(idx).copied()
    }

    pub fn lookup_stop(&self, id: &str) -> Option<usize> {
        self.stop_idx(id)
    }
}

impl Timetable for GtfsTimetable<'_> {
    type Stop = usize;

    type Route = usize;

    type Trip = usize;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        self.routes_for_stops
            .get_or_init(|| self.cache_routes_for_stops())
            .get(&stop)
            .map(|sv| sv.to_vec())
            .unwrap_or_default()
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let stops = self
            .stops_for_routes
            .get_or_init(|| self.cache_stops_for_routes())
            .get(&route)
            .expect("route should exist");

        let left_pos = stops.iter().position(|&s| s == left);
        let right_pos = stops.iter().position(|&s| s == right);

        match (left_pos, right_pos) {
            (Some(l), Some(r)) if l <= r => left,
            (Some(_), Some(_)) => right,
            _ => panic!("both stops should exist on route"),
        }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        let stops = self
            .stops_for_routes
            .get_or_init(|| self.cache_stops_for_routes())
            .get(&route)
            .expect("route should exist");

        let pos = stops
            .iter()
            .position(|&s| s == stop)
            .expect("stop should exist on route");

        stops[pos..].to_vec()
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: crate::Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let trips = self
            .trips_for_routes
            .get_or_init(|| self.cache_trips_for_routes())
            .get(&route)?;

        let stop_id = *self.stops.get(stop)?;

        let departure_at_stop = |trip_idx: usize| -> Option<crate::Tau> {
            let trip_id = self.trips[trip_idx];
            let trip = self.gtfs.get_trip(trip_id).unwrap();
            trip.stop_times
                .iter()
                .find(|st| st.stop.id == stop_id)
                .and_then(|st| st.departure_time)
                .map(|t| t as crate::Tau)
        };

        // Binary search: find partition point where departure >= at
        let idx = trips.partition_point(|&trip_idx| {
            departure_at_stop(trip_idx)
                .map(|dep| dep < at)
                .unwrap_or(true) // trips not serving this stop sort "before"
        });

        // Scan forward to find first trip actually serving this stop
        trips[idx..]
            .iter()
            .find(|&&trip_idx| departure_at_stop(trip_idx).is_some())
            .copied()
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> crate::Tau {
        let trip_id = self.trips[trip];
        let stop_id = self.stops[stop];
        let trip = self.gtfs.get_trip(trip_id).unwrap();

        trip.stop_times
            .iter()
            .find(|st| st.stop.id == stop_id)
            .and_then(|st| st.arrival_time)
            .expect("valid inputs") as crate::Tau
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> crate::Tau {
        let trip_id = self.trips[trip];
        let stop_id = self.stops[stop];
        let trip = self.gtfs.get_trip(trip_id).unwrap();

        trip.stop_times
            .iter()
            .find(|st| st.stop.id == stop_id)
            .and_then(|st| st.departure_time)
            .expect("valid inputs") as crate::Tau
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Vec<Self::Stop> {
        let stop_id = self.stops[stop];

        self.gtfs
            .get_stop(stop_id)
            .unwrap()
            .transfers
            .iter()
            .filter_map(|t| self.stop_idx(&t.to_stop_id))
            .collect()
    }

    // TODO: handle TransferType to distinguish between timed transfers and walking
    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> crate::Tau {
        let from_stop_id = self.stops[from];
        let to_stop_id = self.stops[to];

        self.gtfs
            .get_stop(from_stop_id)
            .unwrap()
            .transfers
            .iter()
            .find(|t| t.to_stop_id == to_stop_id)
            .and_then(|t| t.min_transfer_time)
            .map(|t| t as crate::Tau)
            .unwrap_or(300) // default 5 minutes
    }
}
