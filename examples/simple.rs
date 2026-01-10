use raptor::{Tau, Timetable};

// a single route with stops [0..10]
struct SingleRoute;

impl Timetable for SingleRoute {
    type Stop = usize;

    type Route = usize;

    type Trip = usize;

    fn get_routes_serving_stop(&self, _stop: Self::Stop) -> Vec<Self::Route> {
        vec![0]
    }

    fn get_earlier_stop(
        &self,
        _route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        left.min(right)
    }

    fn get_stops_after(&self, _route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        if stop == 9 {
            return vec![];
        }
        (stop..10).collect()
    }

    fn get_arrival_time(&self, _trip: Self::Trip, stop: Self::Stop) -> Tau {
        stop * 10
    }

    fn get_departure_time(&self, _trip: Self::Trip, stop: Self::Stop) -> Tau {
        (stop * 10) + 5
    }

    fn get_footpaths_from(&self, _stop: Self::Stop) -> Vec<Self::Stop> {
        vec![]
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
    let mock = SingleRoute;

    let journey = mock.raptor(10, 0, 0, 9);

    println!("{journey:#?}");
}
