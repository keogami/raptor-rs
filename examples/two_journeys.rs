use std::borrow::Cow;

use raptor::{Tau, Timetable};

const R1_STOPS: [usize; 4] = [2, 10, 11, 9];
const R0_STOPS: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

struct TwoRoutes;

impl Timetable for TwoRoutes {
    type Stop = usize;

    type Route = &'static str;

    type Trip = usize;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Cow<'static, [Self::Route]> {
        let in_r0 = R0_STOPS.contains(&stop);
        let in_r1 = R1_STOPS.contains(&stop);

        match (in_r0, in_r1) {
            (true, true) => Cow::Borrowed(&["r0", "r1"]),
            (true, false) => Cow::Borrowed(&["r0"]),
            (false, true) => Cow::Borrowed(&["r1"]),
            (false, false) => Cow::Borrowed(&[]),
        }
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        if route == "r0" {
            left.min(right)
        } else {
            let routes = R1_STOPS;

            let left = routes.iter().position(|&a| a == left).unwrap();
            let right = routes.iter().position(|&a| a == right).unwrap();

            routes[left.min(right)]
        }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Cow<'static, [Self::Stop]> {
        if route == "r0" {
            R0_STOPS[stop..].into()
        } else {
            let stop_idx = R1_STOPS.iter().position(|&a| a == stop).unwrap();
            R1_STOPS[stop_idx..].into()
        }
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        if route == "r0" {
            (at < self.get_departure_time(0, stop)).then_some(0)
        } else {
            (at < self.get_departure_time(1, stop)).then_some(1)
        }
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        if trip == 0 {
            stop * 10
        } else {
            let routes = R1_STOPS;
            let stop_idx = routes.iter().position(|&a| a == stop).unwrap();

            (stop_idx + 2) * 10
        }
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        self.get_arrival_time(trip, stop) + 5
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Cow<'static, [Self::Stop]> {
        if stop == 2 {
            [2].as_slice().into()
        } else {
            [].as_slice().into()
        }
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::new().filter_or("RAPTOR_EXAMPLE_LOG_LEVEL", "info"),
    )
    .init();

    let mock = TwoRoutes;

    let journey = mock.raptor(10, 0, 1, 9);

    println!("{journey:#?}");
}
