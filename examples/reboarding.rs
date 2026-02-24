//! Example showing a multi-route RAPTOR query where a passenger reboards
//! a shared route at a later stop reached via a faster feeder route.

use raptor::{Tau, Timetable};

/// Three routes:
/// - R1: S -> A (slow, arrives A @ t=100)
/// - R2: S -> B (fast, arrives B @ t=30)
/// - R3: A -> B -> C -> D (two trips: early and late)
struct ReBoardingTimetable;

impl Timetable for ReBoardingTimetable {
    type Stop = char;
    type Route = &'static str;
    type Trip = u32;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        match stop {
            'S' => vec!["R1", "R2"],
            'A' => vec!["R1", "R3"],
            'B' => vec!["R2", "R3"],
            'C' | 'D' => vec!["R3"],
            _ => vec![],
        }
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let order: &[char] = match route {
            "R1" => &['S', 'A'],
            "R2" => &['S', 'B'],
            "R3" => &['A', 'B', 'C', 'D'],
            _ => return left,
        };
        let l = order.iter().position(|&c| c == left).unwrap_or(99);
        let r = order.iter().position(|&c| c == right).unwrap_or(99);
        order[l.min(r)]
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        let order: &[char] = match route {
            "R1" => &['S', 'A'],
            "R2" => &['S', 'B'],
            "R3" => &['A', 'B', 'C', 'D'],
            _ => return vec![],
        };
        let pos = order.iter().position(|&c| c == stop).unwrap_or(0);
        order[pos..].to_vec()
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        match route {
            "R1" => {
                let dep = match stop {
                    'S' => 0,
                    'A' => 100,
                    _ => return None,
                };
                (at <= dep).then_some(10)
            }
            "R2" => {
                let dep = match stop {
                    'S' => 0,
                    'B' => 30,
                    _ => return None,
                };
                (at <= dep).then_some(20)
            }
            "R3" => {
                let (early_dep, late_dep) = match stop {
                    'A' => (25, 105),
                    'B' => (30, 110),
                    'C' => (40, 120),
                    'D' => (50, 130),
                    _ => return None,
                };
                if at <= early_dep {
                    Some(32)
                } else if at <= late_dep {
                    Some(31)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        match (trip, stop) {
            (10, 'A') => 100,
            (20, 'B') => 30,
            (31, 'B') => 110,
            (31, 'C') => 120,
            (31, 'D') => 130,
            (32, 'B') => 30,
            (32, 'C') => 40,
            (32, 'D') => 50,
            _ => Tau::MAX,
        }
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        match (trip, stop) {
            (10, 'S') => 0,
            (20, 'S') => 0,
            (31, 'A') => 105,
            (31, 'B') => 110,
            (31, 'C') => 120,
            (32, 'A') => 25,
            (32, 'B') => 30,
            (32, 'C') => 40,
            _ => Tau::MAX,
        }
    }

    fn get_footpaths_from(&self, _: Self::Stop) -> Vec<Self::Stop> {
        vec![]
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::new().filter_or("RAPTOR_EXAMPLE_LOG_LEVEL", "info"),
    )
    .init();

    println!("Query: S -> D, departure time 0");
    println!("Expected: S --(R2)--> B --(R3/early)--> D, arrives @ t=50\n");

    let timetable = ReBoardingTimetable;
    let journeys = timetable.raptor(3, 0, 'S', 'D');

    println!("{journeys:#?}");
}
