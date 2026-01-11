use std::collections::HashMap;

use gtfs_structures::Gtfs;

use crate::{Tau, Timetable};

pub type StopIdx = u32;
pub type PatternIdx = u32;
pub type TripIdx = u32;

#[derive(Clone, Copy)]
struct StopTime {
    arrival: u32,
    departure: u32,
}

struct Pattern {
    stops: Vec<StopIdx>,
    stop_positions: HashMap<StopIdx, usize>,
    trips: Vec<TripIdx>,
    departures_by_stop: Vec<Vec<(u32, TripIdx)>>,
}

pub struct GtfsTimetable {
    patterns: Vec<Pattern>,
    stop_to_patterns: Vec<Vec<(PatternIdx, u16)>>,
    trip_stop_times: Vec<StopTime>,
    pattern_offsets: Vec<usize>,
    trip_info: Vec<(PatternIdx, usize)>,

    // could just be a sorted vec, index being internal id, item being the string id
    stop_id_to_idx: HashMap<String, StopIdx>,
    idx_to_stop_id: Vec<String>,
}

struct TripData {
    pattern_idx: PatternIdx,
    first_departure: u32,
    times: Vec<StopTime>,
}

impl GtfsTimetable {
    pub fn from_gtfs(gtfs: &Gtfs) -> Self {
        // Phase 1: Build stop mapping
        let (stop_id_to_idx, idx_to_stop_id) = Self::build_stop_mapping(gtfs);
        let num_stops = idx_to_stop_id.len();

        // Phase 2: Build patterns and collect trip data
        let (mut patterns, trip_data) = Self::build_patterns(gtfs, &stop_id_to_idx);

        // Phase 3: Sort trips within each pattern and build indices
        let (trip_info, total_stop_times) = Self::sort_and_index_trips(&mut patterns, &trip_data);

        // Phase 4: Build stop-to-patterns index
        let stop_to_patterns = Self::build_stop_to_patterns(&patterns, num_stops);

        // Phase 5: Build flat stop times array and departures_by_stop
        let (trip_stop_times, pattern_offsets) =
            Self::build_stop_times(&mut patterns, &trip_data, total_stop_times);

        Self {
            patterns,
            stop_to_patterns,
            trip_stop_times,
            pattern_offsets,
            trip_info,
            stop_id_to_idx,
            idx_to_stop_id,
        }
    }

    fn build_stop_mapping(gtfs: &Gtfs) -> (HashMap<String, StopIdx>, Vec<String>) {
        let mut stop_id_to_idx = HashMap::with_capacity(gtfs.stops.len());
        let mut idx_to_stop_id = Vec::with_capacity(gtfs.stops.len());

        for (idx, stop_id) in gtfs.stops.keys().enumerate() {
            stop_id_to_idx.insert(stop_id.clone(), idx as StopIdx);
            idx_to_stop_id.push(stop_id.clone());
        }

        (stop_id_to_idx, idx_to_stop_id)
    }

    fn build_patterns(
        gtfs: &Gtfs,
        stop_id_to_idx: &HashMap<String, StopIdx>,
    ) -> (Vec<Pattern>, HashMap<String, TripData>) {
        let mut pattern_signatures: HashMap<Vec<StopIdx>, PatternIdx> = HashMap::new();
        let mut patterns: Vec<Pattern> = Vec::new();
        let mut trip_data: HashMap<String, TripData> = HashMap::new();

        for (trip_id, trip) in &gtfs.trips {
            if trip.stop_times.is_empty() {
                continue;
            }

            // Sort stop_times by sequence
            let mut sorted_stop_times: Vec<_> = trip.stop_times.iter().collect();
            sorted_stop_times.sort_by_key(|st| st.stop_sequence);

            // Extract stop sequence
            let stop_sequence: Vec<StopIdx> = sorted_stop_times
                .iter()
                .filter_map(|st| stop_id_to_idx.get(&st.stop.id).copied())
                .collect();

            if stop_sequence.is_empty() {
                continue;
            }

            // Extract times
            let times: Vec<StopTime> = sorted_stop_times
                .iter()
                .map(|st| StopTime {
                    arrival: st.arrival_time.unwrap_or(0),
                    departure: st.departure_time.unwrap_or(0),
                })
                .collect();

            let first_departure = times.first().map(|t| t.departure).unwrap_or(0);

            // Find or create pattern
            let pattern_idx = if let Some(&idx) = pattern_signatures.get(&stop_sequence) {
                idx
            } else {
                let idx = patterns.len() as PatternIdx;
                let stop_positions: HashMap<StopIdx, usize> = stop_sequence
                    .iter()
                    .enumerate()
                    .map(|(pos, &stop)| (stop, pos))
                    .collect();

                patterns.push(Pattern {
                    stops: stop_sequence.clone(),
                    stop_positions,
                    trips: Vec::new(),
                    departures_by_stop: Vec::new(),
                });
                pattern_signatures.insert(stop_sequence, idx);
                idx
            };

            trip_data.insert(
                trip_id.clone(),
                TripData {
                    pattern_idx,
                    first_departure,
                    times,
                },
            );
        }

        (patterns, trip_data)
    }

    fn sort_and_index_trips(
        patterns: &mut [Pattern],
        trip_data: &HashMap<String, TripData>,
    ) -> (Vec<(PatternIdx, usize)>, usize) {
        // Group trips by pattern
        let mut trips_per_pattern: Vec<Vec<(&str, u32)>> = vec![Vec::new(); patterns.len()];

        for (trip_id, data) in trip_data {
            trips_per_pattern[data.pattern_idx as usize]
                .push((trip_id.as_str(), data.first_departure));
        }

        // Sort trips within each pattern by first departure
        for trips in &mut trips_per_pattern {
            trips.sort_by_key(|(_, dep)| *dep);
        }

        // Assign global trip indices and build trip_info
        let total_trips: usize = trip_data.len();
        let mut trip_info = vec![(0 as PatternIdx, 0usize); total_trips];
        let mut trip_id_to_global_idx: HashMap<&str, TripIdx> = HashMap::with_capacity(total_trips);
        let mut global_idx: TripIdx = 0;
        let mut total_stop_times = 0usize;

        for (pattern_idx, trips) in trips_per_pattern.iter().enumerate() {
            let pattern = &mut patterns[pattern_idx];
            pattern.trips = Vec::with_capacity(trips.len());

            for (trip_pos, (trip_id, _)) in trips.iter().enumerate() {
                pattern.trips.push(global_idx);
                trip_info[global_idx as usize] = (pattern_idx as PatternIdx, trip_pos);
                trip_id_to_global_idx.insert(trip_id, global_idx);
                global_idx += 1;
            }

            total_stop_times += trips.len() * pattern.stops.len();
        }

        (trip_info, total_stop_times)
    }

    fn build_stop_to_patterns(
        patterns: &[Pattern],
        num_stops: usize,
    ) -> Vec<Vec<(PatternIdx, u16)>> {
        let mut stop_to_patterns = vec![Vec::new(); num_stops];

        for (pattern_idx, pattern) in patterns.iter().enumerate() {
            for (position, &stop_idx) in pattern.stops.iter().enumerate() {
                stop_to_patterns[stop_idx as usize]
                    .push((pattern_idx as PatternIdx, position as u16));
            }
        }

        stop_to_patterns
    }

    fn build_stop_times(
        patterns: &mut [Pattern],
        trip_data: &HashMap<String, TripData>,
        total_stop_times: usize,
    ) -> (Vec<StopTime>, Vec<usize>) {
        // Calculate offsets
        let mut pattern_offsets = Vec::with_capacity(patterns.len());
        let mut offset = 0;

        for pattern in patterns.iter() {
            pattern_offsets.push(offset);
            offset += pattern.trips.len() * pattern.stops.len();
        }

        // Allocate flat array
        let mut trip_stop_times = vec![
            StopTime {
                arrival: 0,
                departure: 0
            };
            total_stop_times
        ];

        // Build departures_by_stop structure for each pattern
        for pattern in patterns.iter_mut() {
            pattern.departures_by_stop =
                vec![Vec::with_capacity(pattern.trips.len()); pattern.stops.len()];
        }

        // Fill in times and build departures_by_stop
        for (pattern_idx, pattern) in patterns.iter_mut().enumerate() {
            let base_offset = pattern_offsets[pattern_idx];

            // Sort trips by first departure to match pattern.trips order
            let mut sorted_trips: Vec<_> = trip_data
                .iter()
                .filter(|(_, d)| d.pattern_idx as usize == pattern_idx)
                .collect();
            sorted_trips.sort_by_key(|(_, d)| d.first_departure);

            for (trip_pos, (_, data)) in sorted_trips.iter().enumerate() {
                let global_trip_idx = pattern.trips[trip_pos];

                // Copy times to flat array
                for (stop_pos, time) in data.times.iter().enumerate() {
                    let idx = base_offset + trip_pos * pattern.stops.len() + stop_pos;
                    trip_stop_times[idx] = *time;

                    // Add to departures_by_stop
                    pattern.departures_by_stop[stop_pos].push((time.departure, global_trip_idx));
                }
            }

            // Sort departures_by_stop for binary search
            for departures in &mut pattern.departures_by_stop {
                departures.sort_by_key(|(dep, _)| *dep);
            }
        }

        (trip_stop_times, pattern_offsets)
    }

    pub fn get_stop_idx(&self, stop_id: &str) -> Option<StopIdx> {
        self.stop_id_to_idx.get(stop_id).copied()
    }

    pub fn get_stop_id(&self, idx: StopIdx) -> Option<&str> {
        self.idx_to_stop_id.get(idx as usize).map(|s| s.as_str())
    }
}

impl Timetable for GtfsTimetable {
    type Stop = StopIdx;
    type Route = PatternIdx;
    type Trip = TripIdx;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        self.stop_to_patterns
            .get(stop as usize)
            .map(|patterns| patterns.iter().map(|(p, _)| *p).collect())
            .unwrap_or_default()
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let pattern = &self.patterns[route as usize];

        let left_pos = pattern.stop_positions.get(&left);
        let right_pos = pattern.stop_positions.get(&right);

        match (left_pos, right_pos) {
            (Some(&l), Some(&r)) => pattern.stops[l.min(r)],
            (Some(_), None) => left,
            (None, Some(_)) => right,
            (None, None) => left,
        }
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        let pattern = &self.patterns[route as usize];

        pattern
            .stop_positions
            .get(&stop)
            .map(|&pos| pattern.stops[pos..].to_vec())
            .unwrap_or_default()
    }

    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        let pattern = &self.patterns[route as usize];
        let stop_pos = *pattern.stop_positions.get(&stop)?;

        let departures = &pattern.departures_by_stop[stop_pos];
        let at = at as u32;

        // Binary search for first departure >= at
        let pos = departures.partition_point(|(dep, _)| *dep < at);

        departures.get(pos).map(|(_, trip_idx)| *trip_idx)
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        let (pattern_idx, trip_pos) = self.trip_info[trip as usize];
        let pattern = &self.patterns[pattern_idx as usize];

        let stop_pos = match pattern.stop_positions.get(&stop) {
            Some(&pos) => pos,
            None => return Tau::MAX,
        };

        let base = self.pattern_offsets[pattern_idx as usize];
        let idx = base + trip_pos * pattern.stops.len() + stop_pos;

        self.trip_stop_times[idx].arrival as Tau
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        let (pattern_idx, trip_pos) = self.trip_info[trip as usize];
        let pattern = &self.patterns[pattern_idx as usize];

        let stop_pos = match pattern.stop_positions.get(&stop) {
            Some(&pos) => pos,
            None => return Tau::MAX,
        };

        let base = self.pattern_offsets[pattern_idx as usize];
        let idx = base + trip_pos * pattern.stops.len() + stop_pos;

        self.trip_stop_times[idx].departure as Tau
    }

    fn get_footpaths_from(&self, _stop: Self::Stop) -> Vec<Self::Stop> {
        Vec::new()
    }
}
