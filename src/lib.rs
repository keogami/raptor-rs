// TODO: define trait to replicate the raptor's model of the network
// TODO: define the journey struct

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

pub type K = usize;
pub type Tau = usize;

/// Raptor works on a structure called Timetable, which models a route based networks like a metro system's timetable
pub trait Timetable {
    type Stop: Ord + Copy + Debug;
    type Route: Ord + Copy + Debug;
    type Trip: Copy + Debug;

    // TODO: replace vec with cow or iter
    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route>;
    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop;
    // TODO: replace vec with cow or iter
    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop>;
    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip>;
    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau;
    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau;
    // TODO: replace vec with cow or iter
    fn get_footpaths_from(&self, stop: Self::Stop) -> Vec<Self::Stop>;
    fn get_transfer_time(&self, from: Self::Stop, to: Self::Stop) -> Tau {
        let (_, _) = (from, to);
        1
    }

    fn raptor(&self, transfers: usize, tau: usize, ps: Self::Stop, pt: Self::Stop) {
        // for (i, stop) earliest known arrival time at `stop` with at most `i` transfers
        let mut best_arrival_per_k = BTreeMap::<(K, Self::Stop), Tau>::new();
        let mut best_arrival = BTreeMap::<Self::Stop, Tau>::new();

        best_arrival_per_k.insert((0, ps), tau);

        let mut marked_stops = BTreeSet::<Self::Stop>::from([ps]);

        #[allow(non_snake_case)]
        // allowing weird naming to match with the paper
        let mut Q = BTreeMap::<Self::Route, Self::Stop>::new();

        for k in 1..transfers {
            println!("Running for k = {k}");
            println!("marked stops = {marked_stops:#?}");
            Q.clear();
            // find all routes that serve the marked stops, for evaluation in this round
            for &marked_stop in &marked_stops {
                for route in self.get_routes_serving_stop(marked_stop) {
                    let p_dash = Q.entry(route).or_insert(marked_stop);

                    *p_dash = self.get_earlier_stop(route, marked_stop, *p_dash);
                }
            }

            println!("Q after section 1: {Q:#?}");

            marked_stops.clear();

            // scanning each route
            for (&route, &p) in Q.iter() {
                let mut current_trip: Option<Self::Trip> = None;
                println!("scanning route = {route:?} with p = {p:?}");

                for mut pi in self.get_stops_after(route, p) {
                    current_trip = dbg!(current_trip);
                    pi = dbg!(pi);
                    if let Some(arr) = current_trip.map(|trip| self.get_arrival_time(trip, pi)) {
                        let best_arrival_to_target = best_arrival.get(&pt).unwrap_or(&Tau::MAX);
                        let best_arrival_to_pi = best_arrival.get(&pi).unwrap_or(&Tau::MAX);
                        let time_to_beat =
                            *dbg!(best_arrival_to_pi).min(dbg!(best_arrival_to_target));

                        if dbg!(dbg!(arr) < time_to_beat) {
                            best_arrival_per_k.insert((k, pi), arr);
                            best_arrival.insert(pi, arr);
                            marked_stops.insert(pi);
                        }
                    }

                    let t_prev_pi =
                        dbg!(*best_arrival_per_k.get(&(k - 1, pi)).unwrap_or(&Tau::MAX));
                    if t_prev_pi
                        <= dbg!(
                            current_trip
                                .map(|trip| self.get_departure_time(trip, pi))
                                .unwrap_or(Tau::MAX)
                        )
                    {
                        current_trip = self.get_earliest_trip(route, t_prev_pi, pi);
                    }

                    println!("\n")
                }
            }

            // look at footpaths, and mark the stops reachable
            let mut more_marked_stops = Vec::new();
            for &stop in &marked_stops {
                for &p_dash in &self.get_footpaths_from(stop) {
                    let tau = best_arrival_per_k
                        .get(&(k, p_dash))
                        .copied()
                        .unwrap_or(Tau::MAX)
                        .min(
                            best_arrival_per_k
                                .get(&(k, stop))
                                .copied()
                                .unwrap_or(Tau::MAX)
                                + self.get_transfer_time(stop, p_dash),
                        );
                    best_arrival_per_k.insert((k, p_dash), tau);
                    more_marked_stops.push(p_dash);
                }
            }

            marked_stops.extend(&more_marked_stops);

            if marked_stops.is_empty() {
                break;
            }
        }
    }
}
