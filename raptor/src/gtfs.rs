//! [`Timetable`] implementation backed by a GTFS feed.
//!
//! Wraps a parsed [`Gtfs`] object and pre-computes lookup indices
//! for efficient route, stop, and trip queries.

use std::{borrow::Cow, collections::BTreeMap};

use gtfs_structures::Gtfs;
use smallvec::SmallVec;

use crate::Timetable;

const TYPICAL_ROUTES_PER_STOP: usize = 8;
const TYPICAL_STOPS_PER_ROUTE: usize = 32;
const TYPICAL_TRANSFERS_PER_STOP: usize = 4;
const DEFAULT_TRANSFER_TIME_SECONDS: usize = 300;

type RoutesForStops<'gtfs> = BTreeMap<&'gtfs str, SmallVec<[&'gtfs str; TYPICAL_ROUTES_PER_STOP]>>;
type StopForRoutes<'gtfs> = BTreeMap<&'gtfs str, SmallVec<[&'gtfs str; TYPICAL_STOPS_PER_ROUTE]>>;
type TripsForRoutes<'gtfs> = BTreeMap<&'gtfs str, Vec<&'gtfs str>>;
type FootpathsForStops<'gtfs> =
    BTreeMap<&'gtfs str, SmallVec<[&'gtfs str; TYPICAL_TRANSFERS_PER_STOP]>>;

/// Errors that can occur when constructing a [`GtfsTimetable`].
#[derive(thiserror::Error, Debug)]
pub enum GtfsError {
    /// A trip referenced in the feed was not found.
    #[error("trip not found: {0}")]
    MissingTrip(String),
    /// A stop referenced by a trip was not found.
    #[error("stop not found: {0}")]
    MissingStop(String),
    /// A trip has no stop times defined.
    #[error("trip has no stop_times: {0}")]
    MissingStopTimes(String),
}

type GtfsResult<T> = std::result::Result<T, GtfsError>;

/// A [`Timetable`] implementation that wraps a parsed GTFS feed.
///
/// Constructed via [`GtfsTimetable::new`], which validates the feed and builds
/// internal lookup indices for routes, stops, trips, and footpaths.
pub struct GtfsTimetable<'gtfs> {
    gtfs: &'gtfs Gtfs,

    // can use docs.rs/arc-swap's cache for realtime support
    routes_for_stops: RoutesForStops<'gtfs>,
    stops_for_routes: StopForRoutes<'gtfs>,
    trips_for_routes: TripsForRoutes<'gtfs>,
    footpaths_for_stops: FootpathsForStops<'gtfs>,
}

impl<'a> GtfsTimetable<'a> {
    /// Creates a new timetable from a parsed GTFS feed.
    ///
    /// Returns an error if the feed contains trips with missing stops or empty stop times.
    pub fn new(gtfs: &'a Gtfs) -> GtfsResult<Self> {
        let routes_for_stops = Self::cache_routes_for_stops(gtfs)?;
        let stops_for_routes = Self::cache_stops_for_routes(gtfs)?;
        let trips_for_routes = Self::cache_trips_for_routes(gtfs)?;
        let footpaths_for_stops = Self::cache_footpaths_for_stops(gtfs);

        Ok(Self {
            gtfs,
            routes_for_stops,
            stops_for_routes,
            trips_for_routes,
            footpaths_for_stops,
        })
    }

    fn cache_routes_for_stops(gtfs: &'a Gtfs) -> GtfsResult<RoutesForStops<'a>> {
        let mut routes_for_stops = RoutesForStops::default();

        for trip in gtfs.trips.values() {
            if trip.stop_times.is_empty() {
                return Err(GtfsError::MissingStopTimes(trip.id.clone()));
            }

            let route = trip.route_id.as_str();
            for st in &trip.stop_times {
                let stop_id = st.stop.id.as_str();
                gtfs.get_stop(stop_id)
                    .map_err(|_| GtfsError::MissingStop(stop_id.to_owned()))?;
                routes_for_stops.entry(stop_id).or_default().push(route);
            }
        }

        Ok(routes_for_stops)
    }

    fn cache_stops_for_routes(gtfs: &'a Gtfs) -> GtfsResult<StopForRoutes<'a>> {
        let mut stops_for_routes = StopForRoutes::default();

        for trip in gtfs.trips.values() {
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

        Ok(stops_for_routes)
    }

    fn cache_footpaths_for_stops(gtfs: &'a Gtfs) -> FootpathsForStops<'a> {
        let mut footpaths_for_stops = FootpathsForStops::default();

        for (stop_id, stop) in &gtfs.stops {
            if stop.transfers.is_empty() {
                continue;
            }

            let targets: SmallVec<_> = stop
                .transfers
                .iter()
                .map(|t| t.to_stop_id.as_str())
                .collect();
            footpaths_for_stops.insert(stop_id.as_str(), targets);
        }

        footpaths_for_stops
    }

    fn cache_trips_for_routes(gtfs: &'a Gtfs) -> GtfsResult<TripsForRoutes<'a>> {
        let mut trips_for_routes = TripsForRoutes::default();

        for (trip_id, trip) in &gtfs.trips {
            let route = trip.route_id.as_str();
            let trip_idx = trip_id.as_str();
            trips_for_routes.entry(route).or_default().push(trip_idx);
        }

        // Sort each route's trips by first stop departure time
        for trips in trips_for_routes.values_mut() {
            trips.sort_by_key(|&trip_id| {
                let trip = gtfs
                    .get_trip(trip_id)
                    .expect("validated during construction");
                trip.stop_times
                    .first()
                    .expect("validated at construction")
                    .departure_time
            });
        }

        Ok(trips_for_routes)
    }
}

fn find_stop_time<'a>(gtfs: &'a Gtfs, trip: &str, stop: &str) -> &'a gtfs_structures::StopTime {
    let trip = gtfs.get_trip(trip).expect("validated during construction");
    trip.stop_times
        .iter()
        .find(|st| st.stop.id == stop)
        .expect("valid inputs")
}

impl<'gtfs> Timetable for GtfsTimetable<'gtfs> {
    type Stop = &'gtfs str;
    type Route = &'gtfs str;
    type Trip = &'gtfs str;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Cow<'_, [&'gtfs str]> {
        self.routes_for_stops
            .get(&stop)
            .map(|sv| sv.as_slice())
            .unwrap_or(&[])
            .into()
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let stops = self
            .stops_for_routes
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

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'_, [&'gtfs str]> {
        let stops = self
            .stops_for_routes
            .get(&route)
            .expect("route should exist");

        let pos = stops
            .iter()
            .position(|&s| s == stop)
            .expect("stop should exist on route");

        stops[pos..].into()
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: crate::Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let trips = self.trips_for_routes.get(&route)?;

        let departure_at_stop = |trip: &str| -> Option<crate::Tau> {
            let trip = self
                .gtfs
                .get_trip(trip)
                .expect("validated during construction");
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
        find_stop_time(self.gtfs, trip, stop)
            .arrival_time
            .expect("valid inputs") as crate::Tau
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> crate::Tau {
        find_stop_time(self.gtfs, trip, stop)
            .departure_time
            .expect("valid inputs") as crate::Tau
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Cow<'_, [Self::Stop]> {
        self.footpaths_for_stops
            .get(&stop)
            .map(|sv| sv.as_slice())
            .unwrap_or(&[])
            .into()
    }

    // TODO: handle TransferType to distinguish between timed transfers and walking
    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> crate::Tau {
        self.gtfs
            .get_stop(from)
            .expect("validated during construction")
            .transfers
            .iter()
            .find(|t| t.to_stop_id == to)
            .and_then(|t| t.min_transfer_time)
            .map(|t| t as crate::Tau)
            .unwrap_or(DEFAULT_TRANSFER_TIME_SECONDS)
    }
}
