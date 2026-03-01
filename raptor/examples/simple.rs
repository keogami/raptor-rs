use std::borrow::Cow;

use raptor::{Tau, Timetable};

// a single route with stops [0..10]
struct SingleRoute;

impl Timetable for SingleRoute {
    type Stop = usize;

    type Route = usize;

    type Trip = usize;

    fn get_routes_serving_stop(&self, _stop: Self::Stop) -> Cow<'static, [Self::Route]> {
        (&[0]).into()
    }

    fn get_earlier_stop(
        &self,
        _route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        left.min(right)
    }

    fn get_stops_after(&self, _route: Self::Route, stop: Self::Stop) -> Cow<'static, [Self::Stop]> {
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9][stop..].into()
    }

    fn get_arrival_time(&self, _trip: Self::Trip, stop: Self::Stop) -> Tau {
        stop * 10
    }

    fn get_departure_time(&self, _trip: Self::Trip, stop: Self::Stop) -> Tau {
        (stop * 10) + 5
    }

    fn get_footpaths_from(&self, _stop: Self::Stop) -> Cow<'static, [Self::Stop]> {
        (&[]).into()
    }

    fn get_earliest_trip(
        &self,
        _route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        (at < self.get_departure_time(0, stop)).then_some(0)
    }
}

fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::new().filter_or("RAPTOR_EXAMPLE_LOG_LEVEL", "info"),
    )
    .init();

    let mock = SingleRoute;

    let journey = mock.raptor(10, 0, 0, 9);

    println!("{journey:#?}");
}
