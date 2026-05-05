# raptor-rs

Rust implementation of the RAPTOR (Round-bAsed Public Transit Routing) algorithm.

Given a transit network, RAPTOR finds all Pareto-optimal journeys between two
stops – trading off between fewer transfers and earlier arrival.

Based on the paper: [*Round-Based Public Transit Routing*](https://www.microsoft.com/en-us/research/publication/round-based-public-transit-routing/) by Delling, Pajor, and Werneck.

## Concepts

RAPTOR proceeds in *rounds*. Round k holds the earliest known arrival time
at every stop reachable using at most k trips. Each round scans the routes
that touched a stop improved in the previous round, then relaxes walking
footpaths to a fixed point. The output is a Pareto front: one journey per
trip count between `0` and `max_transfers`, with strictly earlier arrivals
as you allow more trips. There is no Dijkstra-style priority queue and no
shortest-path tree – just successive rounds of array updates.

The default routing is *single-criterion*: minimise arrival time, then
report one journey per trip count. For problems where a single best answer
is the wrong shape – "show me a slower route with less walking" – swap in a
multi-criterion `Label` (e.g. the bundled `ArrivalAndWalk`) and the algorithm
returns a real Pareto front trading off the criteria you care about. The
`Label` trait is the only seam in the algorithm where this plugs in; the
core scan is unchanged.

The public surface is small. Implement (or use) the `Timetable` trait to
describe a transit network – `GtfsTimetable` does this for any GTFS feed —
then build queries with `tt.query().from(...).to(...).depart_at(...).run()`.
A `RaptorCache` reuses scratch allocations across queries against the same
timetable, useful for server workloads; a `RaptorCachePool` does the same
across threads, and range queries can fan their per-departure work across
cores via `.run_par()` (under the default-on `parallel` feature). Per-leg
trip and timing reconstruction is opt-in via `journey.with_timing(...)`;
`Journey.plan` on its own is just `(route, alight stop)` topology.

Things you can usually ignore until you need them: the `Label` trait
(default is fine for normal routing); the `Timetable` trait internals
(only relevant if you have non-GTFS data); range queries via
`.depart_in_window(...)` (only if you want a profile rather than one
departure); and `RaptorCache` (only worth it if you run many queries
back-to-back).

## Quick start: query a GTFS feed

The `gtfs` module wraps a parsed GTFS feed (via the
[`gtfs-structures`](https://crates.io/crates/gtfs-structures) crate) and
implements the `Timetable` trait for it. Most users start here.

```rust
use gtfs_structures::Gtfs;
use jiff::civil::date;
use raptor::{SecondOfDay, Timetable};
use raptor::gtfs::GtfsTimetable;

let gtfs = Gtfs::new("path/to/gtfs.zip")?;
// GTFS feeds describe many service days; pin the timetable to one date.
// Trips whose service_id is not active on this day are filtered out.
let timetable = GtfsTimetable::new(&gtfs, date(2026, 5, 4))?;

// The query takes dense u32 indices, not GTFS string IDs – resolve first.
let start = timetable.stop_idx("dilshad_garden").expect("unknown stop");
let target = timetable.stop_idx("vishwavidyalaya").expect("unknown stop");

let journeys = timetable
    .query()
    .from(start)
    .to(target)
    .max_transfers(10)
    .depart_at(SecondOfDay::hms(9, 0, 0))
    .run();

for journey in &journeys {
    print!("arrives {}s, plan: ", journey.arrival());
    for (route_idx, stop_idx) in &journey.plan {
        let route = timetable.route_id(*route_idx);   // original GTFS route_id
        let stop = timetable.stop_id(*stop_idx);      // original GTFS stop_id
        print!("[{route} -> {stop}] ");
    }
    println!();
}
```

A returned `Journey` has `plan: Vec<(RouteIdx, StopIdx)>` (each entry is
"take this route, get off at this stop"), plus `origin` and `target` fields
recording which user-supplied stops the algorithm picked, and a `label: L`
that for the default single-criterion `ArrivalTime` carries the effective
arrival time. Use `journey.arrival()` to read it as a `SecondOfDay` (seconds since
midnight, including the chosen target's walk-time offset). Each returned
journey is Pareto-optimal: arrival strictly decreases as trip count
increases, and no two returned journeys weakly dominate each other.

For station-level queries (where any platform of a parent station is an
acceptable origin or target), `GtfsTimetable::station_stops(parent_id)`
returns the children as a slice ready to pass to `.from(...)` / `.to(...)`:

```rust,ignore
let journeys = timetable
    .query()
    .from(timetable.station_stops("berlin_hbf"))   // 301 platforms
    .to(timetable.station_stops("berlin_alex"))    // 50 platforms
    .max_transfers(10)
    .depart_at(SecondOfDay::hms(9, 0, 0))
    .run();
```

A runnable version of the above is in `examples/gtfs-timetable.rs`:

```bash
cargo run --example gtfs-timetable -- path/to/gtfs.zip start_id target_id
```

## Performance

Single-query latency on the bundled Delhi Metro feed
(`aux/dmrc_gtfs.zip` – 262 stops, 36 routes, 5,438 trips), measured with
[`criterion`](https://crates.io/crates/criterion) on Apple Silicon (M-series),
warm cache reused across iterations:

| Query                                   | Median latency |
|-----------------------------------------|----------------|
| Direct, 1 trip (Dilshad Garden→Shahdara)| 9 µs           |
| 2-trip with one interchange             | 38 µs          |
| 3-trip across three lines               | 73 µs          |

Construction (parsing the GTFS zip + building the indexed timetable)
dominates at ~98 ms – pay it once at startup, then queries are essentially
free. For server workloads doing many queries against the same timetable,
reuse a [`RaptorCache`](#reusing-a-raptorcache) to amortise scratch-buffer
allocation.

To reproduce these numbers on your own hardware:

```bash
cargo bench -p raptor --features gtfs-bench --bench gtfs
```

For numbers on larger feeds – Helsinki HSL (~8k stops), Berlin VBB
(~42k stops), Paris IDFM (~54k stops) – see
[`docs/cross-city-benchmarks.md`](docs/cross-city-benchmarks.md). That
page also documents the real-feed correctness work that the cross-city
run drove (parent-station aggregation, calendar filtering,
transfer-graph density), all of which now have shipped fixes.

## Reading a Journey

A `Journey<L>` has a `plan` and a `label`. The plan is a
`Vec<(RouteIdx, StopIdx)>` – each entry means "take this route, get off at
this stop". The source stop is implicit; it is not part of the plan. The
`label: L` is the algorithm's per-stop label at the target; for the default
single-criterion `ArrivalTime`, `journey.arrival()` (a method) returns the
effective arrival time as a `SecondOfDay`.

The plan records transit boardings only. If the optimal journey ends with a
walk leg from the last boarded alight stop to the target, the plan still
ends at the boarded alight stop and `arrival()` reflects the walk-derived
arrival time at the target. Compare `journey.plan.last()`'s stop with your
target to detect this case.

For custom labels (e.g. tracking accumulated walking time alongside arrival),
see the `Label` trait and `Timetable::query_with_label::<L>()` (which mirrors
`.query()` exactly but returns a `Query<..., L, ...>`). The single-criterion
`ArrivalTime` impl is the default and inlines to plain `SecondOfDay` operations.

The `raptor::labels` module ships canned multi-criterion impls – currently
`ArrivalAndWalk`, which returns a Pareto front of journeys trading off
arrival time against accumulated walking time. Useful for accessibility-aware
queries ("show me a slower route with less walking").

### Per-leg timing

`Journey.plan` is just topology – to recover the specific trips ridden and
their per-leg departure/arrival times, call
`journey.with_timing(&tt, tau, origin_walk)`:

```rust,ignore
for leg in journey.with_timing(&tt, tau, Duration::ZERO).unwrap() {
    println!(
        "board {} on {} at {}s, alight {} at {}s (trip {})",
        leg.board, leg.route, leg.depart, leg.alight, leg.arrive, leg.trip,
    );
}
```

Walking transfers between consecutive transit legs are implicit (the gap in
timestamps reflects the walk). See `TimedLeg`'s docs for details.

### Range queries

For "leave between 17:00 and 18:00 – what are my options?" queries, swap the
builder's `.depart_at(...)` for `.depart_in_window(...)`:

```rust,ignore
use raptor::SecondOfDay;
let profile = tt
    .query()
    .from(start)
    .to(end)
    .max_transfers(10)
    .depart_in_window(SecondOfDay::every(
        SecondOfDay::hms(17, 0, 0),
        SecondOfDay::hms(18, 0, 0),
        60,
    ))
    .run();
for entry in &profile {
    println!("leave at {}, arrive at {}", entry.depart, entry.journey.arrival());
}
```

The returned `Vec<RangeJourney>` is Pareto-optimal on
`(later depart, fewer transfers, dominated label)` – duplicates and
strictly-worse alternatives are dropped automatically. See `RangeJourney`'s
docs for the exact contract. The serial implementation runs rRAPTOR
(paper §4): a single reverse-chronological scan that reuses labels across
departures rather than running N independent RAPTOR calls. The parallel
paths (`.run_par()` / `.run_with_pool(...)`) keep the naïve-batch shape
fanned across cores, since rRAPTOR is inherently sequential within a
window.

To translate index newtypes back to your adapter's external IDs, the bundled
GTFS adapter exposes `GtfsTimetable::stop_id(stop_idx)` and
`GtfsTimetable::route_id(route_idx)`. The synthetic-route splitting (one GTFS
`route_id` may map to several `RouteIdx`s – one per equivalence class of
trips with identical, non-overtaking stop sequences) is documented on
`GtfsTimetable`; `routes_for_gtfs_id(&str)` enumerates the synthetics
derived from a single GTFS route.

## Reusing a `RaptorCache`

A `.run()` call allocates fresh scratch buffers. For server workloads doing
many queries against the same timetable, allocate a `RaptorCache` once and
finish each builder chain with `.run_with_cache(&mut cache)`:

```rust
use raptor::{RaptorCache, SecondOfDay, Timetable};

let mut cache = RaptorCache::for_timetable(&timetable);
for q in queries {
    let journeys = timetable
        .query()
        .from(&q.origins)
        .to(&q.targets)
        .max_transfers(q.transfers)
        .depart_at(q.tau)
        .run_with_cache(&mut cache);
    // ...
}
```

A `RaptorCache` is sized for a specific timetable's `n_stops()`/`n_routes()`.
Calling `.run_with_cache(...)` with a cache whose dimensions differ from the
timetable in scope panics on entry – share a cache only across queries
against the same timetable. If you do not have the timetable in scope at
cache-construction time, `RaptorCache::with_capacity(n_stops, n_routes)` is
the count-only equivalent.

The cache is reset at the start of each call but retains its heap
allocations. A single `RaptorCache` is not thread-safe; give each worker
thread its own — or hand them a shared `RaptorCachePool` (next section).

## Parallel queries

The `parallel` feature (on by default) brings in [Rayon](https://crates.io/crates/rayon)
and unlocks two parallel entry points for range queries:

```rust,ignore
use raptor::{RaptorCachePool, SecondOfDay, Timetable};

// One pool, sized for this timetable, shared across many range queries.
let pool = RaptorCachePool::for_timetable(&timetable);

let profile = timetable
    .query()
    .from(start)
    .to(end)
    .max_transfers(10)
    .depart_in_window(SecondOfDay::every(
        SecondOfDay::hms(17, 0, 0),
        SecondOfDay::hms(18, 0, 0),
        60,
    ))
    .run_with_pool(&pool);   // fans across Rayon's global thread pool
```

`.run_par()` is the no-pool shortcut (allocates an internal pool for the
call, then drops it). Output of `.run_par()` / `.run_with_pool(&pool)` is
identical to the serial `.run()`; only the per-departure work fans out.

`RaptorCachePool` is also useful without Rayon — it's `Sync`, so each
worker thread (or async task) can `.checkout()` a cache for one query and
return it on drop. The same pool serves an arbitrary number of threads
without per-thread bookkeeping.

A measurement on the bundled Delhi feed (60-departure window, 09:00–10:00
in 1-minute steps, M-series Apple Silicon, 8 cores). The serial column is
rRAPTOR (paper §4 reverse-chronological scan), not the naïve batch:

| Query                        | Serial (rRAPTOR) | Parallel (naïve batch) | Speedup |
|------------------------------|------------------|------------------------|---------|
| Direct, 1 trip               | 1.12 ms          | 195 µs                 | 5.7x    |
| 2-trip with one interchange  | 1.35 ms          | 443 µs                 | 3.0x    |
| 3-trip across three lines    | 1.84 ms          | 799 µs                 | 2.3x    |

For wide windows on multicore the parallel path still wins, because
rRAPTOR is sequential within a window. For single-core builds or
narrow windows the serial rRAPTOR path wins.

Compared with the previous naïve serial batch, rRAPTOR is markedly
faster on non-trivial queries — the 2-trip case drops from 2.38 ms to
1.35 ms (~43% faster) and the 3-trip case from 5.37 ms to 1.84 ms
(~66% faster). The 1-trip case is the outlier: it's slightly slower
than the old naïve serial (~547 µs → 1.12 ms) because rRAPTOR's per-τ
overhead — the newly-active-stops scan and the label-bag insert checks
— doesn't amortise on a single-route query. rRAPTOR wins where label
reuse pays off, which is most non-trivial queries.

For a single departure there is nothing to parallelise — `.run()`
(single-departure) is unchanged.

To opt out of Rayon (wasm, embedded, minimal builds), depend on raptor
with `default-features = false`. The `RaptorCachePool` API stays
available; only `.run_par()` / `.run_with_pool(...)` are gated off.

## Implementing `Timetable` for a custom backend

If you have transit data not in GTFS form, implement the `Timetable` trait.
It is non-generic. Identifiers are dense `u32` newtypes – `StopIdx`,
`RouteIdx`, `TripIdx` – and slice-returning accessors return plain `&[T]`
(no `Cow`). Implementors expose two count methods – `n_stops()` and
`n_routes()` – alongside the lookup methods the algorithm calls in its hot
loop. Adapters are responsible for interning external identifiers (e.g. GTFS
string IDs) to dense indices at construction.

One contract the algorithm relies on:

- **Trips on a route must share a stop sequence and not overtake.** The
  algorithm binary-searches by departure time at intermediate stops. If your
  data source groups trips with different stop patterns or overtaking pairs
  under one route, split them at construction. The bundled GTFS adapter does
  this automatically.

Footpaths returned by `get_footpaths_from` describe direct walks only – they
do not need to be transitively closed. The algorithm relaxes footpaths to a
fixed point per round (multi-source Dijkstra), so chained walks `A → B → C`
are reached automatically with the combined walk time.

For adapters whose footpath relation *is* transitively closed (typically a
publisher-curated `transfers.txt` treated as the entire intended relation),
override `Timetable::footpaths_are_transitively_closed` to return `true`. The
algorithm then uses a single-pass `O(E)` relaxation instead of Dijkstra's
`O(E log V)` heap. `GtfsTimetable` ships with this returning `false` by
default; opt in via `GtfsTimetable::assert_footpaths_closed()`.

Both points are documented on the `Timetable` trait.

### Walking footpaths from coordinates

For GTFS feeds whose `transfers.txt` is empty or sparse,
`GtfsTimetable::with_walking_footpaths(&gtfs, max_distance_m, walking_speed_m_per_s)`
augments the footpath graph with bidirectional walking edges between every
pair of stops within straight-line `max_distance_m`. It uses an R-tree over
an equirectangular projection (accurate to ~0.5% at city scale) and
preserves any existing `transfers.txt` entries.

```rust,no_run
use jiff::civil::date;
use raptor::gtfs::GtfsTimetable;

let gtfs = gtfs_structures::Gtfs::new("helsinki.zip")?;
let tt = GtfsTimetable::new(&gtfs, date(2026, 5, 4))?
    .with_walking_footpaths(&gtfs, 500.0, 1.4); // 500m at 5 km/h
# Ok::<(), anyhow::Error>(())
```

## License

Apache-2.0
