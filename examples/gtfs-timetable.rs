// Usage: cargo run --example gtfs-timetable <path_to_zip> <start_stop> <target_stop>

use gtfs_structures::Gtfs;
use humantime::format_duration;
use raptor::{gtfs::GtfsTimetable, Journey, Timetable};
use std::{env, time::Duration};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "Usage: {} <path_to_zip> <start_stop> <target_stop>",
            args[0]
        );
        std::process::exit(1);
    }

    let path = &args[1];
    let start_stop_id = &args[2];
    let target_stop_id = &args[3];

    // Load GTFS
    let gtfs = Gtfs::new(path)?;
    let timetable = GtfsTimetable::new(&gtfs);

    // Resolve stop IDs to internal indices
    let start = timetable
        .lookup_stop(start_stop_id)
        .ok_or_else(|| anyhow::anyhow!("Start stop '{}' not found", start_stop_id))?;
    let target = timetable
        .lookup_stop(target_stop_id)
        .ok_or_else(|| anyhow::anyhow!("Target stop '{}' not found", target_stop_id))?;

    // Run RAPTOR (depart at 19:15)
    let departure_time = 19 * 3600 + 15 * 60;
    let journeys = timetable.raptor(10, departure_time, start, target);

    if journeys.is_empty() {
        println!("No journeys found.");
        return Ok(());
    }

    // Pretty print journeys
    for (i, journey) in journeys.iter().enumerate() {
        let travel_time = Duration::from_secs((journey.arrival - departure_time) as u64);
        println!(
            "Journey {} ({}):",
            i + 1,
            format_duration(travel_time)
        );
        print_journey(&timetable, &gtfs, journey, target);
        println!();
    }

    Ok(())
}

fn print_journey(timetable: &GtfsTimetable, gtfs: &Gtfs, journey: &Journey<usize, usize>, target: usize) {
    // Format: "stop_name" -["route_name"]-> "stop_name" ...
    let plan = &journey.plan;

    for (i, (route, boarding_stop)) in plan.iter().enumerate() {
        let stop_id = timetable.resolve_stop(*boarding_stop).unwrap();
        let stop_name = gtfs
            .stops
            .get(stop_id)
            .and_then(|s| s.name.as_deref())
            .unwrap_or(stop_id);

        let route_id = timetable.resolve_route(*route).unwrap();
        let route_name = gtfs
            .routes
            .get(route_id)
            .and_then(|r| r.short_name.as_deref().or(r.long_name.as_deref()))
            .unwrap_or(route_id);

        // Print boarding stop
        print!("\"{}\" -[\"{}\"]-> ", stop_name, route_name);

        // Alighting stop: next boarding stop or target
        let alight_stop = if i + 1 < plan.len() {
            plan[i + 1].1
        } else {
            target
        };

        let alight_id = timetable.resolve_stop(alight_stop).unwrap();
        let alight_name = gtfs
            .stops
            .get(alight_id)
            .and_then(|s| s.name.as_deref())
            .unwrap_or(alight_id);

        if i + 1 == plan.len() {
            println!("\"{}\"", alight_name);
        } else {
            print!("\"{}\" ", alight_name);
        }
    }
}
