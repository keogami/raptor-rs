use raptor::{Tau, Timetable};

// a single route with stops [0..10]
struct TwoRoutes;

impl Timetable for TwoRoutes {
    type Stop = usize;

    type Route = &'static str;

    type Trip = usize;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        let mut routes = vec![];

        if (0..10).contains(&stop) {
            routes.push("r0");
        }

        if [2, 10, 11, 9].contains(&stop) {
            routes.push("r1")
        }

        routes
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
            let routes = [2, 10, 11, 9];

            let left = routes.iter().position(|&a| a == left).unwrap();
            let right = routes.iter().position(|&a| a == right).unwrap();

            routes[left.min(right)]
        }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        if route == "r0" {
            if stop == 9 {
                return vec![];
            }
            (stop..10).collect()
        } else {
            let routes = [2, 10, 11, 9];
            let stop_idx = routes.iter().position(|&a| a == stop).unwrap();

            routes[stop_idx..].to_vec()
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
            let routes = [2, 10, 11, 9];
            let stop_idx = routes.iter().position(|&a| a == stop).unwrap();

            (stop_idx + 2) * 10
        }
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        self.get_arrival_time(trip, stop) + 5
    }

    fn get_footpaths_from(&self, stop: Self::Stop) -> Vec<Self::Stop> {
        if stop == 2 { vec![2] } else { vec![] }
    }
}

fn main() {
    let mock = TwoRoutes;

    let journey = mock.raptor(10, 0, 1, 9);

    println!("{journey:#?}");
}
