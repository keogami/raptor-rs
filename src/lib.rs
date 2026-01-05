// TODO: define trait to replicate the raptor's model of the network
// TODO: define the journey struct

use std::collections::{BTreeMap, BTreeSet};

type K = usize;
type Tau = usize;

/// Raptor works on a structure called Timetable, which models a route based networks like a metro system's timetable
pub trait Timetable {
    type Stop: Ord + Copy;
    type Route: Ord + Copy;
    type Transfer;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route>;
    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop;
    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop>;
    fn get_next_arrival(&self, current_trip: Tau, stop: Self::Stop) -> Tau;

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
            Q.clear();
            for &marked_stop in &marked_stops {
                for route in self.get_routes_serving_stop(marked_stop) {
                    let p_dash = Q.entry(route).or_insert(marked_stop);

                    *p_dash = self.get_earlier_stop(route, marked_stop, *p_dash);
                }
            }

            marked_stops.clear();

            // this section is shaky
            // TODO: confirm the relationship between t_arr(t, p) and et(r, p)
            // and figure out how to model it in this trait
            for (&route, &p) in Q.iter() {
                let mut t: Option<Tau> = None;

                for p_i in self.get_stops_after(route, p) {
                    let best_arrival_to_target = best_arrival.get(&pt).unwrap_or(&usize::MAX);
                    // TODO: redo this stinking ass if
                    if t.is_some_and(|t| {
                        self.get_next_arrival(t, p_i)
                            < *best_arrival
                                .get(&p_i)
                                .unwrap_or(&usize::MAX)
                                .min(best_arrival_to_target)
                    }) {
                        let next_arrival = self.get_next_arrival(t.unwrap(), p_i);
                        best_arrival_per_k.insert((k, p_i), next_arrival);
                        best_arrival.insert(p_i, next_arrival);
                        marked_stops.insert(p_i);
                    }
                }
            }
        }
    }
}
