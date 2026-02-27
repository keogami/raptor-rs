# raptor-rs

Rust implementation of the RAPTOR (Round-bAsed Public Transit Routing) algorithm.

Given a transit network, RAPTOR finds all pareto-optimal journeys between two
stops — trading off between fewer transfers and earlier arrival. It does this
really fast.

Based on the paper: [*Round-Based Public Transit Routing*](https://www.microsoft.com/en-us/research/publication/round-based-public-transit-routing/) by Delling, Pajor, and Werneck.

## Usage

The core of the crate is the `Timetable` trait. Implement it for your transit
data and you get the `raptor` method for free.

```rust
use raptor::Timetable;

let journeys = my_timetable.raptor(
    3,      // max transfers
    28800,  // departure time: 08:00 in seconds
    source, // source stop
    target, // target stop
);

for journey in &journeys {
    println!("arrives at {} with {} step(s)", journey.arrival, journey.plan.len());
}
```

Each journey in the result is pareto-optimal: no other journey arrives earlier
with the same or fewer transfers.

## Reading a Journey

A `Journey` has a `plan` and an `arrival` time. The plan is a list of
(route, stop) pairs — each entry means "take this route, get off at this stop".
The source stop is implicit; it's not part of the plan.

For example, going from stop `"A"` to stop `"D"` with two transfers:

```rust
[("R1", "B"), ("R2", "C"), ("R3", "D")]
```

Read as: board `R1` at `A`, get off at `B`, board `R2` at `B`, get off at `C`,
board `R3` at `C`, get off at `D`.

## GTFS Support

A ready-made implementation for GTFS feeds ships in the `gtfs` module:

```rust
use gtfs_structures::Gtfs;
use raptor::gtfs::GtfsTimetable;
use raptor::Timetable;

let gtfs = Gtfs::from_path("path/to/gtfs.zip").unwrap();
let timetable = GtfsTimetable::new(&gtfs).unwrap();

let journeys = timetable.raptor(10, 69300, "stop_a", "stop_b");
```

There's also a runnable example:

```bash
cargo run --example gtfs-timetable path/to/gtfs.zip stop_a stop_b
```

## License

Apache-2.0
