# raptor-rs

A Rust implementation of the [RAPTOR algorithm](https://www.microsoft.com/en-us/research/publication/round-based-public-transit-routing/) for fast public transit routing.

## Overview

This crate provides:

- **`Timetable` trait** — A generic abstraction for transit timetables, decoupled from any specific data format
- **`GtfsTimetable`** — A ready-to-use implementation backed by [GTFS](https://gtfs.org/) feeds

## Usage

```rust
use gtfs_structures::Gtfs;
use raptor::{gtfs::GtfsTimetable, Timetable};

let gtfs = Gtfs::from_path("path/to/gtfs")?;
let timetable = GtfsTimetable::new(&gtfs);

let source = timetable.lookup_stop("stop_a").unwrap();
let dest = timetable.lookup_stop("stop_b").unwrap();

// Find journeys departing at 8:00 AM with up to 3 transfers
let journeys = timetable.raptor(3, 8 * 3600, source, dest);

for journey in journeys {
    println!("{:?}", journey);
}
```

The provided `raptor()` method returns `Vec<Journey<Route, Stop>>` — a list of optimal journeys with increasing transfer counts. Each `Journey` contains a `plan` (sequence of route/boarding-stop pairs) and the final `arrival` time.

## Citation

```bibtex
@inproceedings{delling2012round-based,
author = {Delling, Daniel and Pajor, Thomas and Werneck, Renato},
title = {Round-Based Public Transit Routing},
booktitle = {Proceedings of the 14th Meeting on Algorithm Engineering and Experiments (ALENEX'12)},
year = {2012},
month = {January},
abstract = {We study the problem of computing all Pareto-optimal journeys in a dynamic public transit network for two criteria: arrival time and number of transfers. Existing algorithms consider this as a graph problem, and solve it using variants of Dijkstra's algorithm. Unfortunately, this leads to either high query times or suboptimal solutions. We take a different approach. We introduce RAPTOR, our novel round-based public transit router. Unlike previous algorithms, it is not Dijkstra-based, looks at each route (such as a bus line) in the network at most once per round, and can be made even faster with simple pruning rules and parallelization using multiple cores. Because it does not rely on preprocessing, RAPTOR works in fully dynamic scenarios. Moreover, it can be easily extended to handle flexible departure times or arbitrary additional criteria, such as fare zones. When run on London's complex public transportation network, RAPTOR computes all Pareto-optimal journeys between two random locations an order of magnitude faster than previous approaches, which easily enables interactive applications.},
publisher = {Society for Industrial and Applied Mathematics},
url = {https://www.microsoft.com/en-us/research/publication/round-based-public-transit-routing/},
edition = {Proceedings of the 14th Meeting on Algorithm Engineering and Experiments (ALENEX'12)},
```
