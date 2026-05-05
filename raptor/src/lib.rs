#![deny(missing_docs)]

//! Rust implementation of [RAPTOR][paper] (Delling, Pajor, Werneck): given a
//! transit network, find all Pareto-optimal journeys between two stops,
//! trading fewer transfers against earlier arrival.
//!
//! [paper]: https://www.microsoft.com/en-us/research/publication/round-based-public-transit-routing/
//!
//! # Quick start
//!
//! Most users start with the bundled [`gtfs`] adapter, which wraps a parsed
//! GTFS feed and implements [`Timetable`] for it.
//!
//! ```no_run
//! use gtfs_structures::Gtfs;
//! use jiff::civil::date;
//! use raptor::{SecondOfDay, Timetable};
//! use raptor::gtfs::GtfsTimetable;
//!
//! # fn main() -> anyhow::Result<()> {
//! let gtfs = Gtfs::new("path/to/gtfs.zip")?;
//! // Pin the timetable to one service date; inactive trips are filtered out.
//! let tt = GtfsTimetable::new(&gtfs, date(2026, 5, 4))?;
//!
//! // The algorithm takes dense u32 indices, not GTFS string IDs — resolve first.
//! let start = tt.stop_idx("dilshad_garden").expect("unknown stop");
//! let target = tt.stop_idx("vishwavidyalaya").expect("unknown stop");
//!
//! let journeys = tt
//!     .query()
//!     .from(start)
//!     .to(target)
//!     .max_transfers(10)
//!     .depart_at(SecondOfDay::hms(9, 0, 0))
//!     .run();
//!
//! for j in &journeys {
//!     println!("{} trip(s), arrives {}", j.plan.len(), j.arrival());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Concepts
//!
//! RAPTOR proceeds in *rounds*. Round k holds the earliest known arrival time
//! at every stop reachable using at most k trips. Each round scans the routes
//! through stops improved in the previous round and then relaxes walking
//! footpaths to a fixed point. The output is a Pareto front: one [`Journey`]
//! per trip count between zero and `max_transfers`, with strictly earlier
//! arrivals as you allow more trips. There is no Dijkstra-style priority queue
//! and no shortest-path tree — just successive rounds of array updates.
//!
//! The default routing is *single-criterion*: minimise arrival time, then
//! report one journey per trip count. The criterion under optimisation is
//! abstracted by the [`Label`] trait; the default [`ArrivalTime`] just carries
//! a [`SecondOfDay`]. Multi-criterion routing (e.g. trade-offs between arrival
//! time and accumulated walking time) plugs in via [`Timetable::query_with_label`]
//! with one of the [`labels`] impls or your own.
//!
//! # Common patterns
//!
//! - **Single query.** [`Timetable::query`] returns a typestate builder.
//!   Chain `.from(...).to(...).max_transfers(...).depart_at(...).run()`.
//! - **Multi-platform stations.** [`gtfs::GtfsTimetable::station_stops`]
//!   expands a parent station to its child platforms; pass it to `.from(...)` /
//!   `.to(...)`.
//! - **Range queries.** [`Query::depart_in_window`] returns a
//!   [`Vec<RangeJourney>`](RangeJourney) Pareto profile across departure times.
//!   Serial `.run()` is rRAPTOR; `.run_par()` / `.run_with_pool(&pool)` fan
//!   per-departure work across Rayon (default-on `parallel` feature).
//! - **Server reuse.** Allocate a [`RaptorCache`] once via
//!   [`RaptorCache::for_timetable`] and finish each chain with
//!   `.run_with_cache(&mut cache)` — amortises scratch-buffer allocation
//!   across queries.
//! - **Multi-threaded reuse.** [`RaptorCachePool`] is the `Sync` variant.
//!   Each worker calls [`RaptorCachePool::checkout`] to get a cache for one
//!   query; the cache returns to the pool on drop.
//! - **Multi-criterion routing.** [`Timetable::query_with_label`] takes a
//!   custom [`Label`]. The bundled [`labels::ArrivalAndWalk`] trades arrival
//!   time against accumulated walking time.
//! - **Per-leg trip IDs and timing.** [`Journey::with_timing`] reconstructs
//!   per-leg `(boarding stop, trip, depart, alight stop, arrive)` tuples.
//!   `Journey.plan` on its own is just topology.
//! - **Sparse `transfers.txt`.** [`gtfs::GtfsTimetable::with_walking_footpaths`]
//!   builds bidirectional walking edges from stop coordinates using an R-tree.
//!
//! # Custom backends
//!
//! Implement the [`Timetable`] trait directly if your data is not in GTFS
//! form. Identifiers are dense `u32` newtypes ([`StopIdx`], [`RouteIdx`],
//! [`TripIdx`]); your adapter interns external IDs to dense indices at
//! construction. The trait carries one mandatory soundness contract —
//! no-overtaking within a route — documented on [`Timetable`]. Footpaths
//! returned by [`Timetable::get_footpaths_from`] describe direct walks only;
//! the algorithm chains them within a round, so transitive closure is not
//! required. Adapters whose relation *is* closed can opt into a faster
//! single-pass relaxation via [`Timetable::footpaths_are_transitively_closed`].
//!
//! [`manual::SimpleTimetable`] is a hand-rolled in-memory adapter useful for
//! tests and small fixtures.
//!
//! # Errors
//!
//! The library uses three patterns, picked per-call to match what the
//! caller can usefully do:
//!
//! - **`Result` for construction.** Building a [`gtfs::GtfsTimetable`]
//!   from a parsed feed validates many invariants and returns
//!   [`Result<_, gtfs::GtfsError>`](gtfs::GtfsError). Custom adapters
//!   should follow the same pattern with their own typed error enum.
//! - **`Option` for lookups.** Resolving an external ID
//!   ([`gtfs::GtfsTimetable::stop_idx`], [`gtfs::GtfsTimetable::route_idx`])
//!   returns `Option` — `None` simply means "not in this timetable".
//!   Same for any "find me an `X` matching `Y`" accessor; there's no
//!   useful structured error.
//! - **`Result` for plan reconstruction.** [`Journey::with_timing`]
//!   returns [`Result<_, TimingError>`] — failure modes (no boarding
//!   stop reachable, no catchable trip, alighting stop not on route)
//!   are surfaced as variants because they carry useful debug context.
//! - **Panics for programmer-violated invariants.** Passing a
//!   [`RaptorCache`] sized for one timetable to a query against a
//!   differently-sized timetable panics — the mistake is unrecoverable
//!   and the assertion message is the diagnostic. Custom [`Timetable`]
//!   adapters that violate the no-overtaking contract documented on
//!   the trait will likely produce wrong answers rather than panic.
//!
//! There is no all-encompassing `raptor::Error` enum — each failure
//! lives at a clear boundary, so unifying them would lose information
//! rather than add it.
//!
//! # Cargo features
//!
//! - `parallel` (default-on) — pulls in `rayon`, enables [`Query::run_par`] /
//!   [`Query::run_with_pool`]. Opt out with `default-features = false` for
//!   wasm or minimal builds; [`RaptorCachePool`] itself stays available.
//! - `gtfs-bench` — enables the `gtfs` criterion benchmark.
//! - `internal` — enables the `raptor` criterion benchmark over [`manual`].

mod algorithm;
mod cache;
mod endpoints;
mod ids;
mod journey;
mod label;
mod time;
mod timetable;

use crate::algorithm::per_call::run_per_call_query;
use crate::algorithm::range::filter_range_pareto_front;
use crate::algorithm::range::raptor_range_rrap_arrival;

pub mod gtfs;
pub mod labels;
/// In-memory `Timetable` adapter you build by hand with `.route(...)` /
/// `.footpath(...)` calls. Useful when your data isn't from a parsed
/// GTFS feed — for tests, custom data sources, and toy examples.
pub mod manual;

pub use cache::PooledCache;
pub use cache::RaptorCache;
pub use cache::RaptorCachePool;
pub use endpoints::Endpoints;
pub use endpoints::IntoEndpoints;
pub use ids::RouteIdx;
pub use ids::StopIdx;
pub use ids::TripIdx;
pub use journey::Journey;
pub use journey::TimedLeg;
pub use journey::TimingError;
pub use label::ArrivalTime;
pub use label::Label;
pub use time::Duration;
pub use time::SecondOfDay;
pub use time::Transfers;
pub use timetable::Timetable;

#[cfg(test)]
mod test;

/// Internal round-counter type, used only for indexing label arrays.
/// User-facing transfer caps are passed as [`Transfers`].
pub(crate) type K = usize;

/// One entry in a range-query profile: a departure time paired with
/// the [`Journey`] it produces. Returned by [`Query::run`] /
/// [`Query::run_with_cache`] when the builder was configured with
/// [`Query::depart_in_window`].
#[derive(Debug, Clone)]
pub struct RangeJourney<L: Label = ArrivalTime> {
    /// The departure time this journey assumes — the user leaves the
    /// origin (or starts the origin walk) at this time.
    pub depart: SecondOfDay,
    /// The journey itself, as if `depart` had been passed to
    /// [`Query::depart_at`] directly.
    pub journey: Journey<L>,
}

// ── Query builder ──────────────────────────────────────────────────

/// Marker type: a `Query` that hasn't yet had a departure configured.
/// `.run()` is not callable in this state; call [`Query::depart_at`]
/// (single departure) or [`Query::depart_in_window`] (range query)
/// first.
#[derive(Debug, Clone, Copy)]
pub struct NeedsDeparture;

/// Marker type: a single-departure `Query`. `.run()` returns
/// `Vec<Journey<L>>`.
#[derive(Debug, Clone, Copy)]
pub struct SingleDeparture {
    at: SecondOfDay,
}

/// Marker type: a range-query `Query` over a window of departure
/// times. The supplied iterator is collected eagerly at builder time
/// and normalised to a descending, deduplicated `Vec<SecondOfDay>`
/// (the order rRAPTOR scans them in). `.run()` returns
/// `Vec<RangeJourney<L>>`.
#[derive(Debug, Clone)]
pub struct RangeDeparture {
    departures: Vec<SecondOfDay>,
}

/// Builder for a RAPTOR query. Constructed via [`Timetable::query`].
///
/// Typestate enforces the construction order:
///
/// - The initial state ([`NeedsDeparture`]) admits the optional-input
///   methods ([`Query::from`], [`Query::to`], [`Query::max_transfers`])
///   plus a single departure-mode transition
///   ([`Query::depart_at`] or [`Query::depart_in_window`]).
/// - Once a departure mode is set ([`SingleDeparture`] or
///   [`RangeDeparture`]) the only further method is [`Query::run`] —
///   plus [`Query::run_with_cache`] for explicit cache reuse. The
///   return type of `.run()` matches the departure mode.
/// - `Query<L, RangeDeparture>` for custom `L != ArrivalTime` exposes
///   only `.run_par()` / `.run_with_pool()` (with the `parallel`
///   feature). The serial rRAPTOR specialisation only fires for
///   `L = ArrivalTime`.
///
/// Type parameters:
///
/// - `'tt`: borrow of the [`Timetable`].
/// - `T`: the timetable type.
/// - `L`: the [`Label`] type. Defaults to [`ArrivalTime`]. Pass a
///   different label by entering via [`Timetable::query_with_label`].
/// - `M`: the typestate marker. Defaults to [`NeedsDeparture`].
#[derive(Debug, Clone)]
pub struct Query<'tt, T, L = ArrivalTime, M = NeedsDeparture>
where
    T: Timetable + ?Sized,
    L: Label,
{
    pub(crate) tt: &'tt T,
    pub(crate) origins: Endpoints,
    pub(crate) targets: Endpoints,
    pub(crate) max_transfers: Transfers,
    pub(crate) mode: M,
    pub(crate) _label: std::marker::PhantomData<L>,
}

// ----- Stage 1: NeedsDeparture — optional inputs and mode transitions -----

impl<'tt, T, L> Query<'tt, T, L, NeedsDeparture>
where
    T: Timetable + ?Sized,
    L: Label,
{
    /// Set the origin endpoints. Replaces any previously-set origins.
    /// Accepts any [`IntoEndpoints`] shape (single stop, slice, vec).
    pub fn from(mut self, ep: impl IntoEndpoints) -> Self {
        self.origins = ep.into_endpoints();
        self
    }

    /// Set the target endpoints. Replaces any previously-set targets.
    pub fn to(mut self, ep: impl IntoEndpoints) -> Self {
        self.targets = ep.into_endpoints();
        self
    }

    /// Cap the number of transit boardings the algorithm explores.
    /// The default is 10. Pass an integer literal — `.max_transfers(10)`
    /// works directly, no suffix needed.
    pub fn max_transfers(mut self, n: u8) -> Self {
        self.max_transfers = Transfers(n);
        self
    }

    /// Configure a single-departure query. After this call, `.run()`
    /// returns `Vec<Journey<L>>`.
    pub fn depart_at(self, t: impl Into<SecondOfDay>) -> Query<'tt, T, L, SingleDeparture> {
        Query {
            tt: self.tt,
            origins: self.origins,
            targets: self.targets,
            max_transfers: self.max_transfers,
            mode: SingleDeparture { at: t.into() },
            _label: std::marker::PhantomData,
        }
    }

    /// Configure a range query over the supplied departure times.
    /// The iterator is collected eagerly at builder time and normalised
    /// to descending, deduplicated order (the order rRAPTOR scans them
    /// in; the parallel naïve-batch path is order-insensitive). After
    /// this call, `.run()` returns `Vec<RangeJourney<L>>` (for
    /// `L = ArrivalTime` via the rRAPTOR specialisation; for other
    /// labels only the parallel paths are exposed).
    pub fn depart_in_window(
        self,
        deps: impl IntoIterator<Item = SecondOfDay>,
    ) -> Query<'tt, T, L, RangeDeparture> {
        let mut departures: Vec<SecondOfDay> = deps.into_iter().collect();
        // rRAPTOR processes descending; sort+dedupe once at builder
        // time so both serial and parallel paths see canonical input.
        departures.sort_unstable_by(|a, b| b.cmp(a));
        departures.dedup();
        Query {
            tt: self.tt,
            origins: self.origins,
            targets: self.targets,
            max_transfers: self.max_transfers,
            mode: RangeDeparture { departures },
            _label: std::marker::PhantomData,
        }
    }
}

// ----- Stage 2a: SingleDeparture — terminal `.run()` -----

impl<'tt, T, L> Query<'tt, T, L, SingleDeparture>
where
    T: Timetable + Sized,
    L: Label,
{
    /// Execute the query, allocating a fresh [`RaptorCache`].
    pub fn run(self) -> Vec<Journey<L>> {
        let mut cache = RaptorCache::<L>::for_timetable(self.tt);
        self.run_with_cache(&mut cache)
    }

    /// Execute the query, reusing `cache`.
    ///
    /// # Panics
    ///
    /// Panics if `cache` was sized for a different timetable
    /// (`tt.n_stops()` or `tt.n_routes()` mismatch). Use
    /// [`RaptorCache::for_timetable`] with the same timetable you call
    /// the query on to avoid this.
    pub fn run_with_cache(self, cache: &mut RaptorCache<L>) -> Vec<Journey<L>> {
        run_per_call_query(
            self.tt,
            cache,
            self.max_transfers.0 as usize,
            self.mode.at,
            self.origins,
            self.targets,
        )
    }
}

// ----- Stage 2b: RangeDeparture, ArrivalTime — rRAPTOR -----

impl<'tt, T> Query<'tt, T, ArrivalTime, RangeDeparture>
where
    T: Timetable + Sized,
{
    /// Execute the range query, allocating a fresh [`RaptorCache`].
    /// Runs rRAPTOR (single reverse-chronological scan reusing labels
    /// across departures).
    pub fn run(self) -> Vec<RangeJourney<ArrivalTime>> {
        let mut cache = RaptorCache::<ArrivalTime>::for_timetable(self.tt);
        self.run_with_cache(&mut cache)
    }

    /// Execute the range query, reusing `cache`. Runs rRAPTOR.
    ///
    /// # Panics
    ///
    /// Panics if `cache` was sized for a different timetable. See
    /// [`RaptorCache::for_timetable`].
    pub fn run_with_cache(
        self,
        cache: &mut RaptorCache<ArrivalTime>,
    ) -> Vec<RangeJourney<ArrivalTime>> {
        raptor_range_rrap_arrival(
            self.tt,
            cache,
            self.max_transfers.0 as usize,
            &self.mode.departures,
            self.origins,
            self.targets,
        )
    }
}

#[cfg(feature = "parallel")]
impl<'tt, T, L> Query<'tt, T, L, RangeDeparture>
where
    T: Timetable + Sized + Sync,
    L: Label + Send + Sync,
{
    /// Execute the range query in parallel, allocating a fresh
    /// [`RaptorCachePool`]. Per-departure work fans out across Rayon's
    /// global thread pool. Output is identical to [`Self::run`].
    ///
    /// Available with the `parallel` feature (on by default). For
    /// repeated range queries against the same timetable, prefer
    /// [`Self::run_with_pool`] to amortise pool construction.
    pub fn run_par(self) -> Vec<RangeJourney<L>> {
        let pool = RaptorCachePool::<L>::for_timetable(self.tt);
        self.run_with_pool(&pool)
    }

    /// Execute the range query in parallel, reusing caches from `pool`.
    ///
    /// # Panics
    ///
    /// Panics if `pool` was sized for a different timetable
    /// (mismatch surfaces in the per-departure scan via
    /// [`RaptorCache::for_timetable`]'s sizing assertion).
    pub fn run_with_pool(self, pool: &RaptorCachePool<L>) -> Vec<RangeJourney<L>> {
        use rayon::prelude::*;

        let transfers = self.max_transfers.0 as usize;
        let origins = self.origins;
        let targets = self.targets;
        let tt = self.tt;
        let departures = self.mode.departures;

        let all: Vec<RangeJourney<L>> = departures
            .par_iter()
            .flat_map_iter(|&depart| {
                let mut cache = pool.checkout();
                let journeys =
                    run_per_call_query(tt, &mut *cache, transfers, depart, &origins, &targets);
                journeys
                    .into_iter()
                    .map(move |j| RangeJourney { depart, journey: j })
            })
            .collect();

        filter_range_pareto_front(all)
    }
}
