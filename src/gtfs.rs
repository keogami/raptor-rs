use std::{collections::BTreeMap, sync::OnceLock};

use gtfs_structures::Gtfs;
use smallvec::SmallVec;

use crate::Timetable;

const TYPICAL_ROUTES_PER_STOP: usize = 8;
const TYPICAL_STOPS_PER_ROUTE: usize = 32;

type RoutesForStops<'gtfs> = BTreeMap<&'gtfs str, SmallVec<[&'gtfs str; TYPICAL_ROUTES_PER_STOP]>>;
type StopForRoutes<'gtfs> = BTreeMap<&'gtfs str, SmallVec<[&'gtfs str; TYPICAL_STOPS_PER_ROUTE]>>;
type TripsForRoutes<'gtfs> = BTreeMap<&'gtfs str, Vec<&'gtfs str>>;

#[derive(thiserror::Error, Debug)]
pub enum GtfsError {
    // empty
}

type GtfsResult<T> = std::result::Result<T, GtfsError>;

pub struct GtfsTimetable<'gtfs> {
    gtfs: &'gtfs Gtfs,

    // can use docs.rs/arc-swap's cache for realtime support
    routes_for_stops: OnceLock<RoutesForStops<'gtfs>>,
    stops_for_routes: OnceLock<StopForRoutes<'gtfs>>,
    trips_for_routes: OnceLock<TripsForRoutes<'gtfs>>,
}

impl<'a> GtfsTimetable<'a> {
    pub fn new(gtfs: &'a Gtfs) -> GtfsResult<Self> {
        Ok(Self {
            gtfs,
            routes_for_stops: Default::default(),
            stops_for_routes: Default::default(),
            trips_for_routes: Default::default(),
        })
    }

    fn cache_routes_for_stops(&self) -> RoutesForStops<'a> {
        let mut routes_for_stops = RoutesForStops::default();

        for trip in self.gtfs.trips.values() {
            let route = trip.route_id.as_str();
            for st in &trip.stop_times {
                let stop = st.stop.id.as_str();
                routes_for_stops.entry(stop).or_default().push(route);
            }
        }

        routes_for_stops
    }

    fn cache_stops_for_routes(&self) -> StopForRoutes<'a> {
        let mut stops_for_routes = StopForRoutes::default();

        for trip in self.gtfs.trips.values() {
            let route = trip.route_id.as_str();
            // TODO: handle case where multiple trips run on a route but with different patterns
            // which require merging stops in a meaningful way

            if stops_for_routes.contains_key(&route) {
                continue;
            }

            for st in &trip.stop_times {
                let stop = st.stop.id.as_str();
                stops_for_routes.entry(route).or_default().push(stop);
            }
        }

        stops_for_routes
    }

    fn cache_trips_for_routes(&self) -> TripsForRoutes<'a> {
        let mut trips_for_routes = TripsForRoutes::default();

        for (trip_id, trip) in &self.gtfs.trips {
            let route = trip.route_id.as_str();
            let trip_idx = trip_id.as_str();
            trips_for_routes.entry(route).or_default().push(trip_idx);
        }

        // Sort each route's trips by first stop departure time
        for trips in trips_for_routes.values_mut() {
            trips.sort_by_key(|&trip_id| {
                let trip = self.gtfs.get_trip(trip_id).unwrap();
                trip.stop_times
                    .first()
                    .and_then(|st| st.departure_time)
                    .unwrap_or(u32::MAX)
            });
        }

        trips_for_routes
    }
}

impl<'gtfs> Timetable for GtfsTimetable<'gtfs> {
    type Stop = &'gtfs str;
    type Route = &'gtfs str;
    type Trip = &'gtfs str;

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

        let departure_at_stop = |trip: &str| -> Option<crate::Tau> {
            let trip = self.gtfs.get_trip(trip).unwrap();
            trip.stop_times
                .iter()
                .find(|st| st.stop.id == stop)
                .and_then(|st| st.departure_time)
                .map(|t| t as crate::Tau)
        };

        // Binary search: find partition point where departure >= at
        let idx = trips.partition_point(|&trip| {
            departure_at_stop(trip).map(|dep| dep < at).unwrap_or(true) // trips not serving this stop sort "before"
        });

        // Scan forward to find first trip actually serving this stop
        trips[idx..]
            .iter()
            .find(|&&trip| departure_at_stop(trip).is_some())
            .copied()
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> crate::Tau {
        let trip = self.gtfs.get_trip(trip).unwrap();

        trip.stop_times
            .iter()
            .find(|st| st.stop.id == stop)
            .and_then(|st| st.arrival_time)
            .expect("valid inputs") as crate::Tau
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> crate::Tau {
        let trip = self.gtfs.get_trip(trip).unwrap();

        trip.stop_times
            .iter()
            .find(|st| st.stop.id == stop)
            .and_then(|st| st.departure_time)
            .expect("valid inputs") as crate::Tau
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Vec<Self::Stop> {
        self.gtfs
            .get_stop(stop)
            .unwrap()
            .transfers
            .iter()
            .map(|t| t.to_stop_id.as_str())
            .collect()
    }

    // TODO: handle TransferType to distinguish between timed transfers and walking
    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> crate::Tau {
        self.gtfs
            .get_stop(from)
            .unwrap()
            .transfers
            .iter()
            .find(|t| t.to_stop_id == to)
            .and_then(|t| t.min_transfer_time)
            .map(|t| t as crate::Tau)
            .unwrap_or(300) // default 5 minutes
    }
}
