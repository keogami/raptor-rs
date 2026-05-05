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
//! # Cargo features
//!
//! - `parallel` (default-on) — pulls in `rayon`, enables [`Query::run_par`] /
//!   [`Query::run_with_pool`]. Opt out with `default-features = false` for
//!   wasm or minimal builds; [`RaptorCachePool`] itself stays available.
//! - `gtfs-bench` — enables the `gtfs` criterion benchmark.
//! - `internal` — enables the `raptor` criterion benchmark over [`manual`].

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::fmt;
use std::sync::Mutex;

use fixedbitset::FixedBitSet;
use smallvec::SmallVec;

pub mod gtfs;
pub mod labels;
/// In-memory `Timetable` adapter you build by hand with `.route(...)` /
/// `.footpath(...)` calls. Useful when your data isn't from a parsed
/// GTFS feed — for tests, custom data sources, and toy examples.
pub mod manual;

#[cfg(test)]
mod test;

/// Internal round-counter type, used only for indexing label arrays.
/// User-facing transfer caps are passed as [`Transfers`].
pub(crate) type K = usize;

/// A point in time, in seconds since midnight on the timetable's
/// service date. Wraps a `u32` — the day is 86,400 seconds; `u32`
/// covers feed quirks like trips encoded past 24h with room to spare.
///
/// SecondOfDay is a *timestamp*. A *length* of time — walk-time offset,
/// transfer time, dwell time — is a [`Duration`], a distinct type.
/// The trait surface uses both consistently so they can't be
/// silently confused.
///
/// Construct via [`SecondOfDay::ZERO`], [`SecondOfDay::from_secs`], [`SecondOfDay::hms`], or
/// the public-field constructor `SecondOfDay(n)`. Extract via
/// [`SecondOfDay::as_secs`] / [`SecondOfDay::as_hms`]. Arithmetic with [`Duration`]
/// is saturating: `SecondOfDay::MAX + Duration::MAX` stays at `SecondOfDay::MAX`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SecondOfDay(pub u32);

impl SecondOfDay {
    /// Midnight (0 seconds since the start of the service day).
    pub const ZERO: SecondOfDay = SecondOfDay(0);
    /// Sentinel for "unreached". The algorithm uses this internally
    /// for empty `(round, stop)` cells.
    pub const MAX: SecondOfDay = SecondOfDay(u32::MAX);

    /// Construct from a raw seconds-since-midnight count.
    pub const fn from_secs(s: u32) -> Self {
        SecondOfDay(s)
    }

    /// Construct from `(hours, minutes, seconds)`.
    pub const fn hms(h: u32, m: u32, s: u32) -> Self {
        SecondOfDay(h * 3600 + m * 60 + s)
    }

    /// The underlying `u32` — seconds since midnight.
    pub const fn as_secs(self) -> u32 {
        self.0
    }

    /// `(hours, minutes, seconds)` decomposition.
    pub const fn as_hms(self) -> (u32, u32, u32) {
        (self.0 / 3600, (self.0 / 60) % 60, self.0 % 60)
    }

    /// Iterator of departure times from `start` (inclusive) to `end`
    /// (exclusive) at `step`-second intervals. Convenience for the
    /// common range-query input pattern:
    ///
    /// ```
    /// # use raptor::SecondOfDay;
    /// let deps: Vec<_> = SecondOfDay::every(
    ///     SecondOfDay::hms(17, 0, 0),
    ///     SecondOfDay::hms(17, 5, 0),
    ///     60,
    /// ).collect();
    /// assert_eq!(deps.len(), 5);
    /// assert_eq!(deps[0], SecondOfDay::hms(17, 0, 0));
    /// assert_eq!(deps[4], SecondOfDay::hms(17, 4, 0));
    /// ```
    ///
    /// Pass directly to
    /// [`Query::depart_in_window`](crate::Query::depart_in_window).
    pub fn every(
        start: SecondOfDay,
        end: SecondOfDay,
        step: u32,
    ) -> impl Iterator<Item = SecondOfDay> + Clone {
        (start.0..end.0)
            .step_by(step.max(1) as usize)
            .map(SecondOfDay)
    }
}

impl From<u32> for SecondOfDay {
    fn from(s: u32) -> Self {
        SecondOfDay(s)
    }
}

impl fmt::Display for SecondOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::ops::Add<Duration> for SecondOfDay {
    /// `SecondOfDay + Duration` advances the timestamp; saturating on overflow.
    type Output = SecondOfDay;
    fn add(self, d: Duration) -> SecondOfDay {
        SecondOfDay(self.0.saturating_add(d.0))
    }
}

impl std::ops::Sub<SecondOfDay> for SecondOfDay {
    /// `SecondOfDay - SecondOfDay` is the [`Duration`] between them, saturating at
    /// [`Duration::ZERO`] if `self < other`.
    type Output = Duration;
    fn sub(self, other: SecondOfDay) -> Duration {
        Duration(self.0.saturating_sub(other.0))
    }
}

impl std::ops::Sub<Duration> for SecondOfDay {
    /// `SecondOfDay - Duration` rewinds the timestamp; saturating at
    /// [`SecondOfDay::ZERO`] on underflow.
    type Output = SecondOfDay;
    fn sub(self, d: Duration) -> SecondOfDay {
        SecondOfDay(self.0.saturating_sub(d.0))
    }
}

/// A length of time, in seconds. Distinct from [`SecondOfDay`] (a point in
/// time) so the algorithm signatures can express which kind they
/// expect: a walk-time offset, a transfer time, and a dwell time
/// are all `Duration`; an arrival time and a departure time are
/// both `SecondOfDay`.
///
/// Constructed via [`Duration::ZERO`], [`Duration::from_secs`], or
/// the public-field constructor `Duration(n)`. Arithmetic is
/// saturating: `Duration::MAX + Duration` stays at `Duration::MAX`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(pub u32);

impl Duration {
    /// Zero seconds.
    pub const ZERO: Duration = Duration(0);
    /// Maximum representable duration. Used internally as the
    /// "no walk recorded" sentinel for some algorithm paths.
    pub const MAX: Duration = Duration(u32::MAX);

    /// Construct from a raw seconds count.
    pub const fn from_secs(s: u32) -> Self {
        Duration(s)
    }

    /// The underlying `u32` — seconds.
    pub const fn as_secs(self) -> u32 {
        self.0
    }
}

impl From<u32> for Duration {
    fn from(s: u32) -> Self {
        Duration(s)
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::ops::Add<Duration> for Duration {
    type Output = Duration;
    fn add(self, o: Duration) -> Duration {
        Duration(self.0.saturating_add(o.0))
    }
}

/// User-facing transfer cap. The algorithm explores rounds 0
/// through `transfers` inclusive, so `Transfers(10)` lets a journey
/// involve up to 10 boardings. `u8` is plenty — practical journey
/// queries cap at single digits.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Transfers(pub u8);

impl Transfers {
    /// No transfers allowed (only direct journeys).
    pub const ZERO: Transfers = Transfers(0);
    /// Maximum representable transfer cap (255).
    pub const MAX: Transfers = Transfers(u8::MAX);

    /// Construct from a raw `u8`.
    pub const fn new(n: u8) -> Self {
        Transfers(n)
    }

    /// The underlying `u8`.
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<u8> for Transfers {
    fn from(n: u8) -> Self {
        Transfers(n)
    }
}

impl fmt::Display for Transfers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Dense index of a stop within a [`Timetable`]. Indices are in `0..tt.n_stops()`.
///
/// Constructed by adapters at timetable-construction time. Display formats as
/// the bare `u32`; round-trip via [`StopIdx::get`] / [`From<u32>`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StopIdx(u32);

impl StopIdx {
    /// Construct from a raw `u32`. The caller is responsible for the value
    /// being a valid index for the timetable in question.
    pub const fn new(n: u32) -> Self {
        Self(n)
    }
    /// The underlying `u32`.
    pub const fn get(self) -> u32 {
        self.0
    }
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for StopIdx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u32> for StopIdx {
    fn from(n: u32) -> Self {
        Self(n)
    }
}
impl From<StopIdx> for u32 {
    fn from(s: StopIdx) -> Self {
        s.0
    }
}

/// Dense index of a route within a [`Timetable`]. Indices are in `0..tt.n_routes()`.
///
/// In the GTFS adapter, a single GTFS `route_id` may map to multiple
/// `RouteIdx`s — one per equivalence class of trips with identical stop
/// sequences and pairwise non-overtaking schedules. See
/// [`gtfs::GtfsTimetable`] for the splitting rules and lookup APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteIdx(u32);

impl RouteIdx {
    /// Construct from a raw `u32`. The caller is responsible for the value
    /// being a valid index for the timetable in question.
    pub const fn new(n: u32) -> Self {
        Self(n)
    }
    /// The underlying `u32`.
    pub const fn get(self) -> u32 {
        self.0
    }
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for RouteIdx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u32> for RouteIdx {
    fn from(n: u32) -> Self {
        Self(n)
    }
}
impl From<RouteIdx> for u32 {
    fn from(r: RouteIdx) -> Self {
        r.0
    }
}

/// Dense index of a trip within a [`Timetable`]. Indices are in `0..n_trips`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TripIdx(u32);

impl TripIdx {
    /// Construct from a raw `u32`. The caller is responsible for the value
    /// being a valid index for the timetable in question.
    pub const fn new(n: u32) -> Self {
        Self(n)
    }
    /// The underlying `u32`.
    pub const fn get(self) -> u32 {
        self.0
    }
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for TripIdx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<u32> for TripIdx {
    fn from(n: u32) -> Self {
        Self(n)
    }
}
impl From<TripIdx> for u32 {
    fn from(t: TripIdx) -> Self {
        t.0
    }
}

/// A bag of `(stop, walk-time-offset)` pairs used as the origin or
/// target of a query.
///
/// Constructed via the [`IntoEndpoints`] trait, which is implemented
/// for the natural input shapes — a single stop, a slice of stops,
/// a slice of `(stop, duration)` pairs, the owning `Vec` forms.
/// Most callers don't construct `Endpoints` directly; they pass
/// whatever they have to a query method that takes `impl IntoEndpoints`.
///
/// Stored inline up to 50 entries — covers the typical
/// parent-station case (the largest real example we've seen is
/// Berlin Hauptbahnhof at ~301 child platforms, which spills to the
/// heap; everything else stays inline).
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    stops: SmallVec<[(StopIdx, Duration); 50]>,
}

impl Endpoints {
    /// Construct empty.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of `(stop, walk)` pairs.
    pub fn len(&self) -> usize {
        self.stops.len()
    }

    /// True iff this endpoint set has no stops.
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// Borrow the underlying `(stop, walk)` slice.
    pub fn as_slice(&self) -> &[(StopIdx, Duration)] {
        &self.stops
    }

    /// Push a single `(stop, walk)` pair.
    pub fn push(&mut self, stop: StopIdx, walk: Duration) {
        self.stops.push((stop, walk));
    }
}

/// Convert any of the natural query-endpoint inputs into an
/// [`Endpoints`] value the algorithm can consume.
///
/// Implemented for:
/// - `StopIdx` — a single stop, walk-time = 0.
/// - `(StopIdx, Duration)` — a single stop with explicit walk-time.
/// - `&[StopIdx]`, `&[StopIdx; N]` — multiple stops, all walk = 0.
/// - `&[(StopIdx, Duration)]`, `&[(StopIdx, Duration); N]` — multiple
///   stops with explicit walks.
/// - `Vec<StopIdx>`, `Vec<(StopIdx, Duration)>` — owned variants.
/// - `&Endpoints`, `Endpoints` — already-built value, identity.
///
/// For anything else (an iterator, your own collection type),
/// collect into one of the above first or build an [`Endpoints`]
/// directly via [`Endpoints::push`].
pub trait IntoEndpoints {
    /// Convert to an [`Endpoints`] value.
    fn into_endpoints(self) -> Endpoints;
}

impl IntoEndpoints for StopIdx {
    fn into_endpoints(self) -> Endpoints {
        let mut e = Endpoints::new();
        e.push(self, Duration::ZERO);
        e
    }
}

impl IntoEndpoints for (StopIdx, Duration) {
    fn into_endpoints(self) -> Endpoints {
        let mut e = Endpoints::new();
        e.push(self.0, self.1);
        e
    }
}

impl IntoEndpoints for &[StopIdx] {
    fn into_endpoints(self) -> Endpoints {
        let mut e = Endpoints::new();
        for &s in self {
            e.push(s, Duration::ZERO);
        }
        e
    }
}

impl<const N: usize> IntoEndpoints for &[StopIdx; N] {
    fn into_endpoints(self) -> Endpoints {
        (self.as_slice()).into_endpoints()
    }
}

impl IntoEndpoints for &[(StopIdx, Duration)] {
    fn into_endpoints(self) -> Endpoints {
        let mut e = Endpoints::new();
        for &p in self {
            e.push(p.0, p.1);
        }
        e
    }
}

impl<const N: usize> IntoEndpoints for &[(StopIdx, Duration); N] {
    fn into_endpoints(self) -> Endpoints {
        (self.as_slice()).into_endpoints()
    }
}

impl IntoEndpoints for Vec<StopIdx> {
    fn into_endpoints(self) -> Endpoints {
        (self.as_slice()).into_endpoints()
    }
}

impl IntoEndpoints for &Vec<StopIdx> {
    fn into_endpoints(self) -> Endpoints {
        (self.as_slice()).into_endpoints()
    }
}

impl IntoEndpoints for Vec<(StopIdx, Duration)> {
    fn into_endpoints(self) -> Endpoints {
        let mut e = Endpoints::new();
        for p in self {
            e.push(p.0, p.1);
        }
        e
    }
}

impl IntoEndpoints for &Vec<(StopIdx, Duration)> {
    fn into_endpoints(self) -> Endpoints {
        (self.as_slice()).into_endpoints()
    }
}

impl IntoEndpoints for &Endpoints {
    fn into_endpoints(self) -> Endpoints {
        self.clone()
    }
}

impl IntoEndpoints for Endpoints {
    fn into_endpoints(self) -> Endpoints {
        self
    }
}

/// A label attached to a `(round, stop)` cell during the RAPTOR scan.
///
/// **Most users can ignore this trait.** [`Timetable::query`] uses
/// [`ArrivalTime`] (single-criterion: minimise arrival time, fewest
/// transfers), which is what the original RAPTOR paper describes and
/// what almost every routing application wants.
///
/// The trait exists so the algorithm can be reused for *multi-criterion*
/// routing — minimising arrival time *and* something else at the same
/// time, returning a Pareto front of trade-offs. Reach for it when a
/// single "best" answer is the wrong shape: e.g. an accessibility-aware
/// query that should also report the route with less walking, even if
/// it arrives slightly later. The bundled [`labels::ArrivalAndWalk`]
/// is one such impl; see also [`Timetable::query_with_label`] for the
/// builder entry point.
///
/// The algorithm maintains a Pareto front (a *bag* of mutually
/// non-dominated labels) per `(round, stop)`, so multi-criterion impls
/// produce real Pareto fronts at the targets rather than a single
/// tiebroken label. Single-criterion `ArrivalTime` bags stay size 1,
/// with no behaviour change versus a non-bag implementation.
pub trait Label: Copy + std::fmt::Debug {
    /// The "unreached" sentinel. The algorithm initialises every
    /// `(round, stop)` cell to this value before seeding origins.
    const UNREACHED: Self;

    /// Initial label at an origin stop, given the user's departure time.
    fn from_departure(at: SecondOfDay) -> Self;

    /// New label produced by alighting from a trip at this stop with
    /// the given arrival time. `self` is the label at the boarding
    /// stop. For multi-criterion impls, components like accumulated
    /// walking time inherit from `self`.
    fn extend_by_trip(self, arrival: SecondOfDay) -> Self;

    /// New label after walking a footpath of duration `walk_time`.
    fn extend_by_footpath(self, walk_time: Duration) -> Self;

    /// `self` weakly dominates `other` (every criterion of `self` is
    /// at most the corresponding criterion of `other`). The default
    /// implementation uses [`Label::arrival`], which is correct for
    /// single-criterion impls.
    fn dominates(&self, other: &Self) -> bool {
        self.arrival() <= other.arrival()
    }

    /// Effective arrival time at the labelled stop. Used by the
    /// algorithm for target-threshold comparisons and by [`Journey`]
    /// output. Always returns [`SecondOfDay::MAX`] for [`Label::UNREACHED`].
    fn arrival(&self) -> SecondOfDay;
}

/// Single-criterion label = arrival time at a stop. Default `L`
/// throughout the algorithm. Constructing from a `SecondOfDay` is direct;
/// extracting back is `arrival()`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrivalTime(pub SecondOfDay);

impl Label for ArrivalTime {
    const UNREACHED: Self = ArrivalTime(SecondOfDay::MAX);

    #[inline]
    fn from_departure(at: SecondOfDay) -> Self {
        ArrivalTime(at)
    }

    #[inline]
    fn extend_by_trip(self, arrival: SecondOfDay) -> Self {
        ArrivalTime(arrival)
    }

    #[inline]
    fn extend_by_footpath(self, walk_time: Duration) -> Self {
        ArrivalTime(self.0 + walk_time)
    }

    #[inline]
    fn arrival(&self) -> SecondOfDay {
        self.0
    }
}

/// A journey found by the RAPTOR algorithm.
///
/// Each journey consists of a sequence of (route, alight stop) steps and a
/// final label. Multiple journeys may be returned for a single query,
/// representing pareto-optimal trade-offs between fewer transfers and earlier
/// arrival.
///
/// `origin` is whichever of the user-supplied origin stops this journey
/// actually started from — relevant for multi-source queries (e.g. "any
/// platform of this station") where the algorithm picks the best origin
/// internally. Similarly `target` is the target stop reached.
///
/// `L` defaults to [`ArrivalTime`] for single-criterion routing.
#[derive(Debug, Clone)]
pub struct Journey<L: Label = ArrivalTime> {
    /// The origin stop this journey starts from, picked from the
    /// user-supplied origin set.
    pub origin: StopIdx,
    /// The target stop this journey ends at, picked from the user-supplied
    /// target set.
    pub target: StopIdx,
    /// Sequence of steps, each a (route, alight stop) pair.
    ///
    /// The origin stop is implicit — it is not part of the plan. Each entry
    /// means "take this route until this stop". The first step boards at the
    /// origin stop, and each subsequent step boards at the stop where the
    /// previous step got off (possibly via an intermediate footpath).
    pub plan: Vec<(RouteIdx, StopIdx)>,
    /// The label at the target stop, with the target's walk-time offset
    /// already folded in. For [`ArrivalTime`], `label.arrival()` is the
    /// effective arrival time in seconds since midnight.
    pub label: L,
}

impl<L: Label> Journey<L> {
    /// Convenience accessor: `self.label.arrival()`. The effective
    /// arrival time at the chosen target, with the target's
    /// walk-time offset already applied.
    pub fn arrival(&self) -> SecondOfDay {
        self.label.arrival()
    }

    /// Walk the plan against `tt` to recover the specific trip ridden
    /// for each leg, plus per-leg departure and arrival times.
    /// `depart` is the original query departure time and `origin_walk`
    /// is the walk-time offset for `self.origin` from the original
    /// origins slice (typically [`Duration::ZERO`] for single-stop
    /// queries).
    ///
    /// Each returned [`TimedLeg`] reports the route, boarding stop,
    /// alighting stop, the specific [`TripIdx`] caught, and the
    /// boarding / alighting times. If the previous leg's alight
    /// stop is not directly served by the next leg's route, this
    /// scans the previous alight stop's direct footpath neighbours
    /// for one that *is* served, walks there, and uses that as the
    /// next leg's `board`. The walking time advances `depart`
    /// without producing a separate leg — callers detect a walking
    /// transfer by comparing `legs[n].alight` with `legs[n+1].board`.
    ///
    /// # Errors
    ///
    /// Returns a [`TimingError`] when the plan can't be matched
    /// against `tt`. For a `Journey` produced by the same `tt` and
    /// `depart`, the only realistic failure is
    /// [`TimingError::NoBoardingStop`] when a transfer would need a
    /// walk chain longer than one direct footpath hop (multi-hop
    /// walk reconstruction is not implemented). The other variants
    /// are soundness escape hatches that indicate a programmer
    /// error or a `Journey` that was matched against a different
    /// timetable.
    ///
    /// **Loop routes:** if `route` revisits the boarding stop on
    /// its sequence, this picks the *earliest* qualifying position
    /// (matching what [`Timetable::get_routes_serving_stop`]
    /// reports). For non-loop routes (the common case) this is
    /// unambiguous; for loop-heavy networks the reconstructed trip
    /// matches the algorithm's choice in practice.
    pub fn with_timing<T: Timetable>(
        &self,
        tt: &T,
        depart: SecondOfDay,
        origin_walk: Duration,
    ) -> Result<Vec<TimedLeg>, TimingError> {
        let mut legs = Vec::with_capacity(self.plan.len());
        let mut current_time = depart + origin_walk;
        let mut current_stop = self.origin;

        for (leg, &(route, alight)) in self.plan.iter().enumerate() {
            // Find a stop on `route` reachable from current_stop —
            // either current_stop itself or, failing that, a one-hop
            // footpath neighbour that the route serves. Pick the
            // first matching neighbour in iteration order; for the
            // typical case of one neighbour on the route this is
            // unambiguous.
            let serving_here = tt.get_routes_serving_stop(current_stop);
            let (board, board_pos, walk_time) =
                if let Some(&(_, pos)) = serving_here.iter().find(|(r, _)| *r == route) {
                    (current_stop, pos, Duration::ZERO)
                } else {
                    let mut found = None;
                    for &neighbour in tt.get_footpaths_from(current_stop) {
                        if let Some(&(_, pos)) = tt
                            .get_routes_serving_stop(neighbour)
                            .iter()
                            .find(|(r, _)| *r == route)
                        {
                            let walk = tt.get_transfer_time(current_stop, neighbour);
                            found = Some((neighbour, pos, walk));
                            break;
                        }
                    }
                    found.ok_or(TimingError::NoBoardingStop {
                        leg,
                        route,
                        from_stop: current_stop,
                    })?
                };

            current_time = current_time + walk_time;

            let trip = tt.get_earliest_trip(route, current_time, board_pos).ok_or(
                TimingError::NoCatchableTrip {
                    leg,
                    route,
                    board_pos,
                    at: current_time,
                },
            )?;
            let depart = tt.get_departure_time(trip, board_pos);

            // Find the alight position by scanning forward from board_pos.
            let stops_ahead = tt.get_stops_after(route, board_pos);
            let alight_offset = stops_ahead.iter().position(|&s| s == alight).ok_or(
                TimingError::UnreachableAlight {
                    leg,
                    route,
                    board_pos,
                    alight,
                },
            )?;
            let alight_pos = board_pos + alight_offset as u32;
            let arrive = tt.get_arrival_time(trip, alight_pos);

            legs.push(TimedLeg {
                route,
                board,
                alight,
                trip,
                depart,
                arrive,
            });

            current_time = arrive;
            current_stop = alight;
        }

        Ok(legs)
    }
}

/// Failure modes for [`Journey::with_timing`].
///
/// Each variant carries the leg index where reconstruction stopped (zero-based)
/// and enough context to identify what went wrong. For a `Journey` produced by
/// the same timetable the algorithm ran against, the only variant a caller
/// realistically hits is [`TimingError::NoBoardingStop`] — and only when a
/// transfer needs a walk chain longer than one direct footpath hop. The other
/// two variants surface programmer errors (a `Journey` matched against a
/// different timetable, a custom adapter violating the no-overtaking contract).
#[non_exhaustive]
#[derive(Debug, Clone, thiserror::Error)]
pub enum TimingError {
    /// No stop in the plan-reconstruction frontier (the previous leg's
    /// alight stop or any of its one-hop footpath neighbours) is served
    /// by the route this leg wants to board. Multi-hop walk chains are
    /// not reconstructed — if the original journey actually walked
    /// through more than one intermediate stop, that walk can't be
    /// recovered from `Journey.plan` alone today.
    #[error(
        "leg {leg}: no boarding stop for route {route} reachable from {from_stop} \
         (or any one-hop walk neighbour)"
    )]
    NoBoardingStop {
        /// Zero-based plan index where reconstruction stopped.
        leg: usize,
        /// The route this leg wanted to board.
        route: RouteIdx,
        /// The stop the rider was at when reconstruction stopped (the
        /// previous leg's alight stop, or `self.origin` for `leg == 0`).
        from_stop: StopIdx,
    },
    /// No trip on the boarded route departs at or after the rider's
    /// available time. For a `Journey` produced by the same timetable
    /// this should not happen — surface as a programmer-error escape
    /// hatch.
    #[error(
        "leg {leg}: no trip on route {route} departs at or after {at} from position {board_pos}"
    )]
    NoCatchableTrip {
        /// Zero-based plan index where reconstruction stopped.
        leg: usize,
        /// The route this leg boarded.
        route: RouteIdx,
        /// The position within `route` where the rider boarded.
        board_pos: u32,
        /// The time at which the rider was ready to board.
        at: SecondOfDay,
    },
    /// The boarded route's stop sequence (from the boarding position
    /// onwards) does not include the claimed alighting stop. For a
    /// `Journey` produced by the same timetable this should not
    /// happen — surface as a programmer-error escape hatch.
    #[error("leg {leg}: route {route} does not reach stop {alight} from position {board_pos}")]
    UnreachableAlight {
        /// Zero-based plan index where reconstruction stopped.
        leg: usize,
        /// The route this leg boarded.
        route: RouteIdx,
        /// The position within `route` where the rider boarded.
        board_pos: u32,
        /// The stop the plan claimed the rider alighted at.
        alight: StopIdx,
    },
}

/// One transit leg of a [`Journey`] with reconstructed timing.
/// Produced by [`Journey::with_timing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedLeg {
    /// The route boarded.
    pub route: RouteIdx,
    /// Stop where the rider boards.
    pub board: StopIdx,
    /// Stop where the rider alights.
    pub alight: StopIdx,
    /// The specific trip ridden — the earliest one departing at or
    /// after the rider's available time at `board`.
    pub trip: TripIdx,
    /// Departure time at `board`, in seconds since midnight.
    pub depart: SecondOfDay,
    /// Arrival time at `alight`, in seconds since midnight.
    pub arrive: SecondOfDay,
}

/// One reconstructable step in a journey: either a transit boarding event
/// (route-scan) or a walk along a footpath. Walks do not consume a round —
/// they happen *within* round `k` at the stop they alight on — so the
/// reconstruction logic chains through walk entries without decrementing
/// the round index.
///
/// `parent_arrival` disambiguates which Pareto-optimal label at the
/// parent stop (in the parent stop's bag) was extended to produce
/// the label this step belongs to. For single-criterion `ArrivalTime`
/// the parent bag is size-1, so this field is redundant; for
/// multi-criterion impls it lets reconstruction follow the right
/// label through the bag.
#[derive(Debug, Clone, Copy)]
enum Step {
    Boarded {
        from: StopIdx,
        route: RouteIdx,
        parent_arrival: SecondOfDay,
    },
    Walked {
        from: StopIdx,
        parent_arrival: SecondOfDay,
    },
}

/// Boarding tree key: `(round, stop, label_arrival)`. The third
/// component disambiguates Pareto-optimal labels with distinct
/// arrival times in the same `(round, stop)` bag.
type BoardingTree = BTreeMap<(K, StopIdx, SecondOfDay), Step>;

/// A Pareto front of [`Label`]s at a single `(round, stop)` cell.
/// Backed by `SmallVec<[L; 8]>` — for single-criterion `ArrivalTime`
/// the bag is always size 1 and stays inline; for multi-criterion
/// impls it grows up to 8 inline before spilling.
#[derive(Debug, Clone)]
struct LabelBag<L: Label> {
    items: SmallVec<[L; 8]>,
}

impl<L: Label> LabelBag<L> {
    fn new() -> Self {
        Self {
            items: SmallVec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn iter(&self) -> impl Iterator<Item = &L> {
        self.items.iter()
    }

    /// Try to insert `new`. Returns `true` if added (and removes any
    /// items it strictly dominates); returns `false` if some existing
    /// item weakly dominates `new` (no change).
    fn insert(&mut self, new: L) -> bool {
        for item in &self.items {
            if item.dominates(&new) {
                return false;
            }
        }
        self.items.retain(|item| !new.dominates(item));
        self.items.push(new);
        true
    }

    /// Minimum `arrival()` across the bag, or `SecondOfDay::MAX` if empty.
    fn min_arrival(&self) -> SecondOfDay {
        self.items
            .iter()
            .map(|l| l.arrival())
            .min()
            .unwrap_or(SecondOfDay::MAX)
    }

    fn clear(&mut self) {
        self.items.clear();
    }
}

impl<L: Label> Default for LabelBag<L> {
    fn default() -> Self {
        Self::new()
    }
}

/// Relax footpaths from every stop in `sources` at round `k`, propagating
/// to a fixed point.
///
/// Iteratively improves `labels[k][p_dash]` and the τ\* table
/// (`best_arrival`) until no walk produces a strictly better arrival.
/// Each improvement records a `Walked` step in the boarding tree so that
/// `reconstruct_journey` can later trace back through the walk leg.
///
/// Because relaxation iterates to a fixed point, the trait's footpath
/// relation does **not** need to be transitively closed: if A→B and B→C
/// are both in the relation, the algorithm chains them within a single
/// round and reaches C with the combined walk time.
///
/// Stops that should be added to the marked set for the next round are
/// pushed onto `out`, gated by `pt_threshold` (the current best effective
/// arrival at any target). Caller drains `out` between calls.
/// Single-pass relaxation for adapters that report `true` from
/// [`Timetable::footpaths_are_transitively_closed`]. One walk per
/// source — chained walks are unnecessary because every reachable
/// pair is already a direct edge in the relation.
///
/// `O(E)` per round, no heap. Always sound when the closure
/// precondition holds; produces the same labels and boarding tree as
/// [`relax_footpaths_round`] in that case.
/// Try to insert `(label, step)` into `labels[k][stop]` and the
/// boarding tree. Updates `best_arrival[stop]` and marks the stop
/// in `out` if the new label improves on `pt_threshold`. Returns
/// `true` if any insertion happened.
#[allow(clippy::too_many_arguments)]
fn insert_into_bag<L: Label>(
    labels: &mut [Vec<LabelBag<L>>],
    best_arrival: &mut [LabelBag<L>],
    board_detail: &mut BoardingTree,
    out: &mut Vec<StopIdx>,
    ever_reached: &mut FixedBitSet,
    pt_threshold: SecondOfDay,
    k: K,
    stop: StopIdx,
    label: L,
    step: Step,
) -> bool {
    let added = labels[k][stop.idx()].insert(label);
    if !added {
        return false;
    }
    board_detail.insert((k, stop, label.arrival()), step);
    best_arrival[stop.idx()].insert(label);
    ever_reached.insert(stop.idx());
    if label.arrival() < pt_threshold {
        out.push(stop);
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn relax_footpaths_round_closed<T: Timetable + ?Sized, L: Label>(
    timetable: &T,
    k: K,
    labels: &mut [Vec<LabelBag<L>>],
    best_arrival: &mut [LabelBag<L>],
    board_detail: &mut BoardingTree,
    sources: &FixedBitSet,
    pt_threshold: SecondOfDay,
    out: &mut Vec<StopIdx>,
    ever_reached: &mut FixedBitSet,
) {
    // Walk each source bag's labels once. With closure, chained walks
    // are unnecessary because every reachable pair is already a direct
    // edge in the relation.
    let mut staged: SmallVec<[L; 8]> = SmallVec::new();
    for stop_bit in sources.ones() {
        let stop = StopIdx::new(stop_bit as u32);
        if labels[k][stop.idx()].is_empty() {
            continue;
        }
        // Snapshot the source bag so we don't iterate while mutating
        // labels[k] (which can include the source itself).
        staged.clear();
        staged.extend(labels[k][stop.idx()].iter().copied());

        for &p_dash in timetable.get_footpaths_from(stop) {
            let walk = timetable.get_transfer_time(stop, p_dash);
            for source_label in &staged {
                let via_walk = source_label.extend_by_footpath(walk);
                insert_into_bag(
                    labels,
                    best_arrival,
                    board_detail,
                    out,
                    ever_reached,
                    pt_threshold,
                    k,
                    p_dash,
                    via_walk,
                    Step::Walked {
                        from: stop,
                        parent_arrival: source_label.arrival(),
                    },
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn relax_footpaths_round<T: Timetable + ?Sized, L: Label>(
    timetable: &T,
    k: K,
    labels: &mut [Vec<LabelBag<L>>],
    best_arrival: &mut [LabelBag<L>],
    board_detail: &mut BoardingTree,
    sources: &FixedBitSet,
    pt_threshold: SecondOfDay,
    out: &mut Vec<StopIdx>,
    heap: &mut BinaryHeap<Reverse<(SecondOfDay, u32)>>,
    ever_reached: &mut FixedBitSet,
) {
    // Multi-source Dijkstra over the footpath graph at round `k`,
    // ordered by `LabelBag::min_arrival()`. Each source's initial
    // priority is its current bag's minimum arrival; transfer times
    // are non-negative so Dijkstra is sound. Uses lazy deletion —
    // stale heap entries are skipped on pop.
    //
    // For multi-criterion bags this propagates only the min-arrival
    // label per stop; non-min labels in a source bag don't drive
    // further relaxation. This is sound for arrival-time pruning but
    // may miss Pareto-optimal walk chains where a non-min-arrival
    // label dominates downstream — a v0.12 concern.
    heap.clear();
    for bit in sources.ones() {
        let min_arr = labels[k][bit].min_arrival();
        if min_arr != SecondOfDay::MAX {
            heap.push(Reverse((min_arr, bit as u32)));
        }
    }

    let mut staged: SmallVec<[L; 8]> = SmallVec::new();
    while let Some(Reverse((arrival, stop_bit))) = heap.pop() {
        let stop = StopIdx::new(stop_bit);
        let stop_min = labels[k][stop.idx()].min_arrival();
        // Skip stale entries — a strictly better label was popped earlier.
        if arrival > stop_min {
            continue;
        }
        staged.clear();
        staged.extend(labels[k][stop.idx()].iter().copied());

        for &p_dash in timetable.get_footpaths_from(stop) {
            let walk = timetable.get_transfer_time(stop, p_dash);
            let mut any_added = false;
            for source_label in &staged {
                let via_walk = source_label.extend_by_footpath(walk);
                if insert_into_bag(
                    labels,
                    best_arrival,
                    board_detail,
                    out,
                    ever_reached,
                    pt_threshold,
                    k,
                    p_dash,
                    via_walk,
                    Step::Walked {
                        from: stop,
                        parent_arrival: source_label.arrival(),
                    },
                ) {
                    any_added = true;
                }
            }
            if any_added {
                heap.push(Reverse((
                    labels[k][p_dash.idx()].min_arrival(),
                    p_dash.get(),
                )));
            }
        }
    }
}

/// Returns the minimum of `best_arrival[t].min_arrival() + w` across
/// all `(t, w)` in `targets`, saturating on overflow. Returns
/// `SecondOfDay::MAX` if every target is unreached.
fn best_to_any_target<L: Label>(
    best_arrival: &[LabelBag<L>],
    targets: &[(StopIdx, Duration)],
) -> SecondOfDay {
    targets
        .iter()
        .map(|&(t, w)| best_arrival[t.idx()].min_arrival() + w)
        .min()
        .unwrap_or(SecondOfDay::MAX)
}

/// Reconstruct a single candidate plan terminating at the target
/// label `(pt, target_arrival)` at round `k`. Traces back through
/// the boarding tree (which is keyed on `(round, stop, label_arrival)`
/// to disambiguate Pareto-optimal labels in the same bag). Returns
/// `Some((origin, plan))` if the trace reaches some stop in `origins`,
/// `None` otherwise.
fn reconstruct_journey(
    tree: &BoardingTree,
    origins: &FixedBitSet,
    pt: StopIdx,
    target_arrival: SecondOfDay,
    k: K,
) -> Option<(StopIdx, Vec<(RouteIdx, StopIdx)>)> {
    if tree.is_empty() {
        return None;
    }

    let mut plan = Vec::with_capacity(k);
    let mut parent = pt;
    let mut parent_arrival = target_arrival;
    let mut inner_k = k;
    // Defensive bound to avoid pathological loops; 100 walk-hops per
    // round is well beyond anything realistic.
    let mut budget = (k + 1) * 100;

    while !origins.contains(parent.idx()) && budget > 0 {
        budget -= 1;

        let Some(step) = tree.get(&(inner_k, parent, parent_arrival)).copied() else {
            break;
        };

        match step {
            Step::Boarded {
                from,
                route,
                parent_arrival: pa,
            } => {
                plan.push((route, parent));
                parent = from;
                parent_arrival = pa;
                if inner_k == 0 {
                    break;
                }
                inner_k -= 1;
            }
            Step::Walked {
                from,
                parent_arrival: pa,
            } => {
                parent = from;
                parent_arrival = pa;
                // walks do not consume a round
            }
        }
    }

    if !plan.is_empty() && origins.contains(parent.idx()) {
        plan.reverse();
        Some((parent, plan))
    } else {
        None
    }
}

/// Reconstruct one Pareto-front-of-trip-counts worth of journeys
/// for the given targets at the current cache state. Used by both
/// the per-call algorithm (after running rounds) and the rRAPTOR
/// scan (snapshot at end of each τ scan).
///
/// Walks each target × round × label-bag entry, traces back via
/// `reconstruct_journey`, and applies the target's walk-time offset.
/// Output is unfiltered (caller applies any Pareto front filtering).
fn extract_target_journeys<L: Label>(
    labels: &[Vec<LabelBag<L>>],
    board_detail: &BoardingTree,
    origin_set: &FixedBitSet,
    targets: &[(StopIdx, Duration)],
    transfers: usize,
) -> Vec<Journey<L>> {
    let mut journeys: Vec<Journey<L>> = Vec::new();
    for &(target, walk) in targets {
        #[allow(clippy::needless_range_loop)]
        for k in 1..=transfers {
            for raw_label in labels[k][target.idx()].iter() {
                let raw_arr = raw_label.arrival();
                if raw_arr == SecondOfDay::MAX {
                    continue;
                }
                let Some((origin, plan)) =
                    reconstruct_journey(board_detail, origin_set, target, raw_arr, k)
                else {
                    continue;
                };
                let label = raw_label.extend_by_footpath(walk);
                journeys.push(Journey {
                    origin,
                    target,
                    plan,
                    label,
                });
            }
        }
    }
    journeys
}

/// Run round-0 footpath relaxation followed by rounds `1..=transfers`
/// against an already-seeded cache. Shared by the per-call algorithm
/// (which seeds a single τ) and the rRAPTOR scan (which re-seeds for
/// each τ in descending order); the only difference between callers
/// is the outer τ-loop that handles seeding.
///
/// **Caller invariants on entry:**
///   - `marked_stops` contains the round-0 starting set: at minimum
///     the sources, plus (for rRAPTOR) any stops where new trips just
///     became catchable at the current τ.
///   - `labels[0]` is seeded at every source with that source's
///     departure label; the corresponding `best_arrival` and
///     `ever_reached` bits are set.
///   - `best_arrival` and `ever_reached` are coherent with `labels`
///     (the helper reads and writes both).
///
/// On exit, `marked_stops` is empty (drained by the early-out check
/// or by the natural marked-stops migration through rounds).
#[allow(clippy::too_many_arguments)]
fn run_raptor_rounds<T: Timetable + ?Sized, L: Label>(
    tt: &T,
    labels: &mut [Vec<LabelBag<L>>],
    best_arrival: &mut [LabelBag<L>],
    board_detail: &mut BoardingTree,
    marked_stops: &mut FixedBitSet,
    q_entry: &mut [Option<u32>],
    q_routes: &mut Vec<RouteIdx>,
    walked_buf: &mut Vec<StopIdx>,
    relax_heap: &mut BinaryHeap<Reverse<(SecondOfDay, u32)>>,
    ever_reached: &mut FixedBitSet,
    transfers: usize,
    targets: &[(StopIdx, Duration)],
) {
    let mut pt_threshold = best_to_any_target(best_arrival, targets);

    // Pick the per-round footpath relaxation strategy once. Closed
    // graphs use a single-pass O(E) walk; non-closed graphs need
    // multi-source Dijkstra to chain walks to a fixed point.
    let footpaths_closed = tt.footpaths_are_transitively_closed();

    if footpaths_closed {
        relax_footpaths_round_closed(
            tt,
            0,
            labels,
            best_arrival,
            board_detail,
            marked_stops,
            pt_threshold,
            walked_buf,
            ever_reached,
        );
    } else {
        relax_footpaths_round(
            tt,
            0,
            labels,
            best_arrival,
            board_detail,
            marked_stops,
            pt_threshold,
            walked_buf,
            relax_heap,
            ever_reached,
        );
    }
    for s in walked_buf.drain(..) {
        marked_stops.insert(s.idx());
    }
    pt_threshold = best_to_any_target(best_arrival, targets);

    for k in 1..=transfers {
        // Sparse carry-forward: only clone bags at stops ever
        // reached so far this query. For real feeds the reached
        // set is small (<10% of stops in typical city queries),
        // so this avoids the 50k-stop dense memcpy that would
        // otherwise dominate per-round overhead.
        let (prev_labels, this_labels) = labels.split_at_mut(k);
        let src = &prev_labels[k - 1];
        let dst = &mut this_labels[0];
        for bit in ever_reached.ones() {
            dst[bit] = src[bit].clone();
        }

        // Build the route queue for this round. Each entry pairs a
        // route with the earliest position on that route from which we
        // can board this round. Stored positions are folded with `min`
        // so multiple marked stops on the same route resolve to the
        // earliest one.
        for stop_bit in marked_stops.ones() {
            let marked_stop = StopIdx::new(stop_bit as u32);
            for &(route, pos) in tt.get_routes_serving_stop(marked_stop) {
                match q_entry[route.idx()] {
                    None => {
                        q_entry[route.idx()] = Some(pos);
                        q_routes.push(route);
                    }
                    Some(prev_pos) => {
                        if pos < prev_pos {
                            q_entry[route.idx()] = Some(pos);
                        }
                    }
                }
            }
        }

        marked_stops.clear();

        // Per-route Pareto bag of riding entries. Each entry =
        // (boarding_label, current_trip, boarding_stop); a single
        // entry suffices for `ArrivalTime` (size-1 stop bags) but
        // multi-criterion impls may carry multiple Pareto-optimal
        // entries that boarded different trips.
        let mut route_bag: SmallVec<[(L, TripIdx, StopIdx); 8]> = SmallVec::new();
        let mut staged: SmallVec<[L; 8]> = SmallVec::new();

        for &route in q_routes.iter() {
            let p_pos = q_entry[route.idx()].expect("route in q_routes must have an entry");
            route_bag.clear();

            for (offset, &pi) in tt.get_stops_after(route, p_pos).iter().enumerate() {
                let pos = p_pos + offset as u32;

                // 1. Alight every active riding entry at pi.
                for &(boarding_label, trip, boarding_stop) in route_bag.iter() {
                    let arr = tt.get_arrival_time(trip, pos);
                    let best_to_pi = best_arrival[pi.idx()].min_arrival();
                    let time_to_beat = best_to_pi.min(pt_threshold);
                    if arr >= time_to_beat {
                        continue;
                    }
                    let new_label = boarding_label.extend_by_trip(arr);
                    if labels[k][pi.idx()].insert(new_label) {
                        best_arrival[pi.idx()].insert(new_label);
                        board_detail.insert(
                            (k, pi, new_label.arrival()),
                            Step::Boarded {
                                from: boarding_stop,
                                route,
                                parent_arrival: boarding_label.arrival(),
                            },
                        );
                        marked_stops.insert(pi.idx());
                        ever_reached.insert(pi.idx());
                    }
                }

                // 2. Try to extend route_bag with labels from
                //    labels[k-1][pi] that can catch a trip on this
                //    route at pi. Snapshot first to avoid aliasing.
                staged.clear();
                staged.extend(labels[k - 1][pi.idx()].iter().copied());
                for candidate in &staged {
                    let cand_arr = candidate.arrival();
                    let trip = match tt.get_earliest_trip(route, cand_arr, pos) {
                        Some(t) => t,
                        None => continue,
                    };
                    let trip_dep = tt.get_departure_time(trip, pos);

                    // Redundancy check: existing route_bag entry
                    // dominates candidate AND boards an at-or-earlier
                    // trip at pi → candidate is redundant.
                    let mut redundant = false;
                    for &(l_existing, t_existing, _) in route_bag.iter() {
                        let existing_dep = tt.get_departure_time(t_existing, pos);
                        if l_existing.dominates(candidate) && existing_dep <= trip_dep {
                            redundant = true;
                            break;
                        }
                    }
                    if redundant {
                        continue;
                    }

                    // Remove entries strictly dominated by candidate.
                    route_bag.retain(|&mut (l_existing, t_existing, _)| {
                        let existing_dep = tt.get_departure_time(t_existing, pos);
                        !(candidate.dominates(&l_existing) && trip_dep <= existing_dep)
                    });
                    route_bag.push((*candidate, trip, pi));
                }
            }
        }

        // Sparse-set reset of the route queue.
        for r in q_routes.drain(..) {
            q_entry[r.idx()] = None;
        }

        // Refresh target threshold after the route scan, then run
        // footpath relax. Refresh again after the footpath round so
        // the next round's boarding decisions use a current threshold.
        pt_threshold = best_to_any_target(best_arrival, targets);
        if footpaths_closed {
            relax_footpaths_round_closed(
                tt,
                k,
                labels,
                best_arrival,
                board_detail,
                marked_stops,
                pt_threshold,
                walked_buf,
                ever_reached,
            );
        } else {
            relax_footpaths_round(
                tt,
                k,
                labels,
                best_arrival,
                board_detail,
                marked_stops,
                pt_threshold,
                walked_buf,
                relax_heap,
                ever_reached,
            );
        }
        for s in walked_buf.drain(..) {
            marked_stops.insert(s.idx());
        }
        pt_threshold = best_to_any_target(best_arrival, targets);

        if marked_stops.is_clear() {
            break;
        }
    }
}

/// Mark stops where any route through them has a trip whose departure
/// time at that position falls in `[lo, hi)`. Used by rRAPTOR between
/// τ scans to find stops that need rescanning because new trips just
/// became catchable.
///
/// `marked` is the destination bitset; bits already set are preserved.
/// One `get_earliest_trip` lookup per (route, position) plus one
/// `get_departure_time` if the lookup hits — overall
/// O(n_routes × max_route_len) calls per invocation, each O(log
/// n_trips_per_route) inside the trait impl.
fn newly_active_stops_into<T: Timetable + ?Sized>(
    tt: &T,
    lo: SecondOfDay,
    hi: SecondOfDay,
    marked: &mut FixedBitSet,
) {
    if lo >= hi {
        return;
    }
    let n_routes = tt.n_routes() as u32;
    for r in 0..n_routes {
        let route = RouteIdx::new(r);
        let stops = tt.get_stops_after(route, 0);
        for (pos_offset, &stop) in stops.iter().enumerate() {
            let pos = pos_offset as u32;
            if let Some(trip) = tt.get_earliest_trip(route, lo, pos)
                && tt.get_departure_time(trip, pos) < hi
            {
                marked.insert(stop.idx());
            }
        }
    }
}

/// Models a route-based transit network for the RAPTOR algorithm.
///
/// Implement this trait to describe your transit network's topology and
/// schedule. The algorithm itself is invoked via the
/// [`Timetable::query`] builder.
///
/// Identifiers are dense `u32` indices ([`StopIdx`], [`RouteIdx`],
/// [`TripIdx`]). Adapters intern from external IDs (e.g. GTFS string IDs)
/// at construction time.
///
/// # Footpaths
///
/// The footpath relation returned by [`get_footpaths_from`] does **not**
/// need to be transitively closed: if you can walk `A → B` and `B → C`,
/// the algorithm will chain them within a single round, reaching `C`
/// from `A` with the combined walk time. Footpath relaxation iterates to
/// a fixed point per round.
///
/// Closure can still be useful as an optimisation — pre-closed graphs
/// have fewer edges to traverse — but it is not a soundness
/// prerequisite.
///
/// # No overtaking within a route
///
/// All trips returned by [`get_earliest_trip`] for a given route must
/// share a stop sequence and pairwise must not overtake. The algorithm
/// uses a binary search by departure time at intermediate stops, which
/// is only sound when the trip ordering is monotone at every stop.
/// Adapters that ingest data with multiple stop patterns or overtaking
/// should split such groups into separate routes at construction.
///
/// [`get_footpaths_from`]: Timetable::get_footpaths_from
/// [`get_earliest_trip`]: Timetable::get_earliest_trip
pub trait Timetable {
    /// Number of stops in this timetable. Stop indices are in `0..n_stops()`.
    fn n_stops(&self) -> usize;
    /// Number of routes (post-pattern-splitting). Route indices are in
    /// `0..n_routes()`.
    fn n_routes(&self) -> usize;

    /// Returns each route serving the given stop, paired with the *earliest*
    /// position of `stop` within that route's sequence.
    ///
    /// For loop routes where `stop` appears more than once on a route, only
    /// the smallest position is reported. Each route appears at most once
    /// in the returned slice.
    fn get_routes_serving_stop(&self, stop: StopIdx) -> &[(RouteIdx, u32)];

    /// Returns the route's stop sequence from `pos` onwards, inclusive.
    ///
    /// Iterating the returned slice with positional offsets gives the
    /// algorithm `(pos + offset, stop_at_position)` pairs without ambiguity,
    /// even when a route revisits stops.
    ///
    /// Panics if `pos` is out of range for the route.
    fn get_stops_after(&self, route: RouteIdx, pos: u32) -> &[StopIdx];

    /// Returns the stop at the given position within a route's sequence.
    ///
    /// Panics if `pos` is out of range for the route.
    fn stop_at(&self, route: RouteIdx, pos: u32) -> StopIdx;

    /// Finds the earliest trip on a route departing at or after `at` from
    /// the stop at the given position within the route's sequence.
    ///
    /// `pos` disambiguates which visit of the stop to consider when the route
    /// revisits it. Returns `None` if no trip departs at or after `at`.
    fn get_earliest_trip(&self, route: RouteIdx, at: SecondOfDay, pos: u32) -> Option<TripIdx>;

    /// Returns the arrival time of a trip at the given position within its
    /// route's sequence.
    fn get_arrival_time(&self, trip: TripIdx, pos: u32) -> SecondOfDay;

    /// Returns the departure time of a trip at the given position within its
    /// route's sequence.
    fn get_departure_time(&self, trip: TripIdx, pos: u32) -> SecondOfDay;

    /// Returns all stops directly reachable from the given stop via
    /// walking (footpaths).
    ///
    /// The relation does not need to be transitively closed — the
    /// algorithm chains walks within a round. See the trait-level docs.
    fn get_footpaths_from(&self, stop: StopIdx) -> &[StopIdx];

    /// Returns the walking transfer time between two stops.
    /// The default implementation returns 1 second.
    fn get_transfer_time(&self, from: StopIdx, to: StopIdx) -> Duration {
        let (_, _) = (from, to);
        Duration(1)
    }

    /// Reports whether the footpath relation is transitively closed —
    /// that is, whether `A → C` is already a direct edge whenever
    /// `A → B` and `B → C` are. The default is `false`.
    ///
    /// When `true`, the algorithm uses a single-pass `O(E)` footpath
    /// relaxation per round instead of the multi-source Dijkstra
    /// fallback (`O(E log V)`). This is a meaningful speedup on
    /// dense closed graphs (e.g. publisher-curated `transfers.txt`
    /// files in Berlin / Paris feeds).
    ///
    /// **Soundness**: returning `true` when the relation is *not*
    /// closed will cause the algorithm to miss journeys whose optimal
    /// path requires chaining direct walks within a round. Only return
    /// `true` if you know the relation is closed.
    fn footpaths_are_transitively_closed(&self) -> bool {
        false
    }

    /// Start a typestate-builder query. Returns a [`Query`] in the
    /// [`NeedsDeparture`] state. Call `.from(...).to(...).max_transfers(...)`
    /// (any order, all optional with defaults), then either
    /// `.depart_at(...)` for a single-departure query or
    /// `.depart_in_window(...)` for a range query, then `.run()`.
    ///
    /// ```no_run
    /// # use raptor::{Timetable, SecondOfDay, Duration, StopIdx};
    /// # fn ex<T: Timetable>(tt: &T, start: StopIdx, end: StopIdx) {
    /// let journeys = tt
    ///     .query()
    ///     .from(start)
    ///     .to(end)
    ///     .max_transfers(10)
    ///     .depart_at(SecondOfDay::hms(9, 0, 0))
    ///     .run();
    /// # }
    /// ```
    fn query(&self) -> Query<'_, Self, ArrivalTime, NeedsDeparture>
    where
        Self: Sized,
    {
        Query {
            tt: self,
            origins: Endpoints::new(),
            targets: Endpoints::new(),
            max_transfers: Transfers(10),
            mode: NeedsDeparture,
            _label: std::marker::PhantomData,
        }
    }

    /// Like [`Timetable::query`] but with a custom [`Label`] type for
    /// multi-criterion routing. `Vec<Journey<L>>` and `Vec<RangeJourney<L>>`
    /// come back from the corresponding `.run()`, with each entry on the
    /// returned Pareto front a different trade-off across `L`'s criteria.
    ///
    /// You only need this if [`ArrivalTime`] (the default) is the wrong
    /// shape for your problem — e.g. you want to surface a slower route
    /// with less walking. The bundled [`labels::ArrivalAndWalk`] does
    /// exactly that. See the [`Label`] trait for what's involved in
    /// writing your own.
    ///
    /// ```no_run
    /// # use raptor::{Timetable, SecondOfDay, StopIdx};
    /// # use raptor::labels::ArrivalAndWalk;
    /// # fn ex<T: Timetable>(tt: &T, start: StopIdx, end: StopIdx) {
    /// let pareto_front = tt
    ///     .query_with_label::<ArrivalAndWalk>()
    ///     .from(start)
    ///     .to(end)
    ///     .max_transfers(10)
    ///     .depart_at(SecondOfDay::hms(9, 0, 0))
    ///     .run();
    /// # }
    /// ```
    fn query_with_label<L: Label>(&self) -> Query<'_, Self, L, NeedsDeparture>
    where
        Self: Sized,
    {
        Query {
            tt: self,
            origins: Endpoints::new(),
            targets: Endpoints::new(),
            max_transfers: Transfers(10),
            mode: NeedsDeparture,
            _label: std::marker::PhantomData,
        }
    }

    /// Implementation entry point for [`Query::run`] /
    /// [`Query::run_with_cache`]. Public-but-hidden so the typestate
    /// builder can dispatch into the algorithm. Don't call this
    /// directly — use [`Timetable::query`] instead.
    #[doc(hidden)]
    fn raptor_with_cache_and_label<L: Label>(
        &self,
        cache: &mut RaptorCache<L>,
        transfers: usize,
        depart: SecondOfDay,
        origins: impl IntoEndpoints,
        targets: impl IntoEndpoints,
    ) -> Vec<Journey<L>>
    where
        Self: Sized,
    {
        let origins = origins.into_endpoints();
        let targets = targets.into_endpoints();
        let origins = origins.as_slice();
        let targets = targets.as_slice();
        cache.reset_for_query(transfers, self.n_stops() as u32, self.n_routes() as u32);
        let RaptorCache {
            labels,
            best_arrival,
            board_detail,
            marked_stops,
            q_entry,
            q_routes,
            walked_buf,
            origin_set,
            relax_heap,
            ever_reached,
            ..
        } = cache;

        // Clear and populate the origin set used by reconstruction.
        origin_set.clear();
        for &(o, _) in origins {
            origin_set.insert(o.idx());
        }

        // Seed labels for each origin at depart + its walk-time offset.
        // Reconstruction breaks the trace loop when it hits an origin
        // (origin_set bit is set), so origins don't need a Step entry.
        for &(o, walk) in origins {
            let t = depart + walk;
            let seed = L::from_departure(t);
            if labels[0][o.idx()].insert(seed) {
                best_arrival[o.idx()].insert(seed);
                marked_stops.insert(o.idx());
                ever_reached.insert(o.idx());
            }
        }

        run_raptor_rounds(
            self,
            labels,
            best_arrival,
            board_detail,
            marked_stops,
            q_entry,
            q_routes,
            walked_buf,
            relax_heap,
            ever_reached,
            transfers,
            targets,
        );

        // For each target stop, enumerate every Pareto-optimal label
        // in the target's bag at every round and reconstruct its plan.
        // Effective label = bag label extended by the target's walk
        // time offset.
        let mut journeys =
            extract_target_journeys(labels, board_detail, origin_set, targets, transfers);

        // Output-side Pareto filter on (trip count, label). For any two
        // returned journeys neither weakly dominates the other on the
        // pair (plan.len, label). For single-criterion `ArrivalTime`
        // this collapses to "strictly decreasing arrival as trip count
        // increases" (the v0.10 contract); for multi-criterion impls
        // it preserves Pareto-incomparable journeys (e.g. faster but
        // more walking vs. slower but less walking).
        //
        // Sorted by plan.len ascending, then by `arrival()` ascending
        // as a tiebreaker so the iteration order is deterministic.
        journeys.sort_by_key(|j| (j.plan.len(), j.arrival()));
        let mut front: Vec<Journey<L>> = Vec::with_capacity(journeys.len());
        'outer: for j in journeys {
            for f in &front {
                if f.plan.len() <= j.plan.len() && f.label.dominates(&j.label) {
                    continue 'outer;
                }
            }
            front.retain(|f| !(j.plan.len() <= f.plan.len() && j.label.dominates(&f.label)));
            front.push(j);
        }
        front
    }
}

/// Pareto-profile filter for range-query output: keep entries that are
/// not weakly dominated by another on `(later depart, fewer transfers,
/// dominated label)`. Sort so the entries we'd prefer to keep come
/// first (later departure, fewer transfers, earlier arrival), then
/// sweep with mutual-domination removal.
fn filter_range_pareto_front<L: Label>(mut all: Vec<RangeJourney<L>>) -> Vec<RangeJourney<L>> {
    all.sort_by(|a, b| {
        b.depart
            .cmp(&a.depart)
            .then(a.journey.plan.len().cmp(&b.journey.plan.len()))
            .then(a.journey.arrival().cmp(&b.journey.arrival()))
    });
    let mut front: Vec<RangeJourney<L>> = Vec::with_capacity(all.len());
    'outer: for r in all {
        for f in &front {
            if f.depart >= r.depart
                && f.journey.plan.len() <= r.journey.plan.len()
                && f.journey.label.dominates(&r.journey.label)
            {
                continue 'outer;
            }
        }
        front.retain(|f| {
            !(r.depart >= f.depart
                && r.journey.plan.len() <= f.journey.plan.len()
                && r.journey.label.dominates(&f.journey.label))
        });
        front.push(r);
    }
    front
}

/// Range-query algorithm specialised for `Label = ArrivalTime`. Single
/// reverse-chronological scan that reuses labels across departure
/// events (rRAPTOR, paper §4). For non-`ArrivalTime` labels, the
/// parallel paths ([`Query::run_par`] / [`Query::run_with_pool`])
/// still cover this case via the naïve batch.
///
/// `departures` must be sorted descending and deduped (the
/// `.depart_in_window(...)` builder normalises this).
///
/// **Why descending τ order:** for τ' < τ_prev, a journey valid at
/// τ_prev is also valid at τ' (the same trip with depart ≥ τ_prev ≥
/// τ' is still catchable). So descending order makes each new seed
/// monotonically improve the bag, and no improvement ever needs to
/// be undone.
///
/// **Why labels accumulate (cache reset only once):** the per-call
/// algorithm resets between queries; rRAPTOR resets only at the start
/// of the whole scan. Labels written at scan τ remain valid for any
/// τ' < τ, so persisting them across scans is correct, not stale —
/// and avoids re-deriving information already discovered.
///
/// **Why `best_arrival` accumulates too:** the per-round carry-forward
/// overwrites `labels[k][X]` at the start of each round k, but
/// `best_arrival` is never cleared. This gives the `pt_threshold`
/// pruning a tight bound across τ scans.
fn raptor_range_rrap_arrival<T: Timetable + ?Sized>(
    tt: &T,
    cache: &mut RaptorCache<ArrivalTime>,
    transfers: usize,
    departures: &[SecondOfDay],
    origins: Endpoints,
    targets: Endpoints,
) -> Vec<RangeJourney<ArrivalTime>> {
    let origins = origins.as_slice();
    let targets = targets.as_slice();

    cache.reset_for_query(transfers, tt.n_stops() as u32, tt.n_routes() as u32);
    let RaptorCache {
        labels,
        best_arrival,
        board_detail,
        marked_stops,
        q_entry,
        q_routes,
        walked_buf,
        origin_set,
        relax_heap,
        ever_reached,
        ..
    } = cache;

    // origin_set is constant across τ scans; populate once.
    origin_set.clear();
    for &(source, _) in origins {
        origin_set.insert(source.idx());
    }

    let mut output: Vec<RangeJourney<ArrivalTime>> = Vec::new();
    let mut prev_tau: Option<SecondOfDay> = None;

    for &tau in departures {
        // (a) Seed sources at τ + walk. `LabelBag::insert` returns
        // true iff the new label strictly improves the bag; for τ <
        // prev_tau the new label dominates the prior one, so
        // re-seeding tightens the source bag. For the first scan
        // there's nothing to dominate and the seed is added directly.
        marked_stops.clear();
        for &(source, walk) in origins {
            let seed = ArrivalTime(tau + walk);
            if labels[0][source.idx()].insert(seed) {
                best_arrival[source.idx()].insert(seed);
                marked_stops.insert(source.idx());
                ever_reached.insert(source.idx());
            }
        }

        // (b) Mark stops with newly-catchable trips for this τ. The
        // window is half-open `[tau, prev_tau)` — trips departing at
        // exactly `tau` are first-catchable in this scan, while trips
        // at `prev_tau` were already covered by the previous scan.
        if let Some(prev) = prev_tau {
            newly_active_stops_into(tt, tau, prev, marked_stops);
        }

        // (c) Round-0 footpath relax + rounds 1..=transfers, sharing
        // labels with previous scans.
        run_raptor_rounds(
            tt,
            labels,
            best_arrival,
            board_detail,
            marked_stops,
            q_entry,
            q_routes,
            walked_buf,
            relax_heap,
            ever_reached,
            transfers,
            targets,
        );

        // (d) Snapshot per-target journeys for this τ.
        let snapshot =
            extract_target_journeys(labels, board_detail, origin_set, targets, transfers);
        for journey in snapshot {
            output.push(RangeJourney {
                depart: tau,
                journey,
            });
        }

        prev_tau = Some(tau);
    }

    filter_range_pareto_front(output)
}

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
    tt: &'tt T,
    origins: Endpoints,
    targets: Endpoints,
    max_transfers: Transfers,
    mode: M,
    _label: std::marker::PhantomData<L>,
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

    /// Execute the query, reusing `cache`. The cache must be sized
    /// for the same timetable; passing one sized differently panics
    /// inside `RaptorCache::reset_for_query`.
    pub fn run_with_cache(self, cache: &mut RaptorCache<L>) -> Vec<Journey<L>> {
        self.tt.raptor_with_cache_and_label(
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
    /// `pool` must be sized for `self`'s timetable; mismatch panics
    /// inside the per-departure scan.
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
                let journeys = tt.raptor_with_cache_and_label(
                    &mut *cache,
                    transfers,
                    depart,
                    &origins,
                    &targets,
                );
                journeys
                    .into_iter()
                    .map(move |j| RangeJourney { depart, journey: j })
            })
            .collect();

        filter_range_pareto_front(all)
    }
}

/// Reusable scratch buffers for [`Query::run_with_cache`].
///
/// `Query::run` allocates a fresh cache on every call. For workloads that
/// run many queries against the same timetable (a server, a batch job),
/// allocate a `RaptorCache` once and pass `&mut cache` to
/// [`Query::run_with_cache`] at the end of each builder chain — the
/// timetable-sized buffers get reset rather than reallocated between queries.
///
/// ```no_run
/// # use raptor::{RaptorCache, SecondOfDay, Timetable};
/// # fn ex<T: Timetable>(tt: &T, queries: &[(raptor::StopIdx, raptor::StopIdx)]) {
/// let mut cache = RaptorCache::for_timetable(tt);
/// for &(start, end) in queries {
///     let _ = tt.query()
///         .from(start)
///         .to(end)
///         .depart_at(SecondOfDay::hms(9, 0, 0))
///         .run_with_cache(&mut cache);
/// }
/// # }
/// ```
///
/// A cache is sized for a specific timetable's `n_stops` / `n_routes`;
/// passing it to a query against a differently-sized timetable panics on
/// entry. Use [`RaptorCache::with_capacity`] when the timetable isn't yet in
/// scope.
///
/// `RaptorCache` is `!Sync` — give each worker its own, or use
/// [`RaptorCachePool`] (the `Sync` freelist variant).
pub struct RaptorCache<L: Label = ArrivalTime> {
    n_stops: u32,
    n_routes: u32,

    /// labels[k][stop.idx()] = Pareto bag of labels at stop with at
    /// most k trips. Empty bag means unreached.
    labels: Vec<Vec<LabelBag<L>>>,

    /// τ* — Pareto bag of best labels at each stop across all rounds.
    best_arrival: Vec<LabelBag<L>>,

    /// Boarding tree for journey reconstruction.
    board_detail: BoardingTree,

    /// Bitset of marked stops, sized to n_stops.
    marked_stops: FixedBitSet,

    /// Per-round route queue. `q_entry[r.idx()] = Some(boarding_pos)` when
    /// route r has been entered this round; `q_routes` is the dense list
    /// of routes that have entries (for cheap iteration without scanning
    /// `n_routes` empty slots).
    q_entry: Vec<Option<u32>>,
    q_routes: Vec<RouteIdx>,

    /// Scratch buffer for footpath relaxation output.
    walked_buf: Vec<StopIdx>,

    /// Bitset of origin stops for the current query. Reconstruction
    /// terminates when the trace reaches any bit set here.
    origin_set: FixedBitSet,

    /// Min-heap reused across footpath relaxations. Entries are
    /// `(arrival_time, stop_bit)`; lazy deletion via the time field.
    relax_heap: BinaryHeap<Reverse<(SecondOfDay, u32)>>,

    /// Bitset of stops with a non-empty bag in any round seen so
    /// far this query. Used to make per-round carry-forward sparse:
    /// only clone bags at set bits, not all `n_stops`.
    ever_reached: FixedBitSet,
}

impl<L: Label> RaptorCache<L> {
    /// Constructs a cache sized for the given timetable.
    pub fn for_timetable<T: Timetable + ?Sized>(tt: &T) -> Self {
        Self::with_capacity(tt.n_stops() as u32, tt.n_routes() as u32)
    }

    /// Constructs a cache for a timetable with the given counts. Use
    /// [`for_timetable`](Self::for_timetable) when you have the timetable
    /// in scope.
    pub fn with_capacity(n_stops: u32, n_routes: u32) -> Self {
        Self {
            n_stops,
            n_routes,
            labels: Vec::new(),
            best_arrival: (0..n_stops).map(|_| LabelBag::new()).collect(),
            board_detail: BTreeMap::new(),
            marked_stops: FixedBitSet::with_capacity(n_stops as usize),
            q_entry: vec![None; n_routes as usize],
            q_routes: Vec::new(),
            walked_buf: Vec::new(),
            origin_set: FixedBitSet::with_capacity(n_stops as usize),
            relax_heap: BinaryHeap::new(),
            ever_reached: FixedBitSet::with_capacity(n_stops as usize),
        }
    }

    fn reset_for_query(&mut self, transfers: K, tt_n_stops: u32, tt_n_routes: u32) {
        assert_eq!(
            self.n_stops, tt_n_stops,
            "RaptorCache sized for {} stops but timetable has {}",
            self.n_stops, tt_n_stops
        );
        assert_eq!(
            self.n_routes, tt_n_routes,
            "RaptorCache sized for {} routes but timetable has {}",
            self.n_routes, tt_n_routes
        );

        // Resize labels: (transfers + 1) Vecs, each n_stops long, all empty bags.
        let needed = transfers + 1;
        for v in self.labels.iter_mut() {
            v.iter_mut().for_each(|b| b.clear());
        }
        if self.labels.len() < needed {
            self.labels.resize_with(needed, || {
                (0..self.n_stops).map(|_| LabelBag::new()).collect()
            });
        } else {
            self.labels.truncate(needed);
        }

        for v in &mut self.best_arrival {
            v.clear();
        }

        self.board_detail.clear();
        self.marked_stops.clear();
        self.ever_reached.clear();

        // Sparse-set reset: walk q_routes, clear corresponding q_entry slots.
        for r in self.q_routes.drain(..) {
            self.q_entry[r.idx()] = None;
        }

        self.walked_buf.clear();
    }
}

/// A `Sync` pool of [`RaptorCache`]s sized for one timetable. Hand it
/// to multiple threads (or a Rayon worker pool) and have each thread
/// `checkout()` a cache for the duration of one query.
///
/// Backed by a mutex-protected freelist; the lock is only held long
/// enough to pop or push, never during the query itself. Caches grow
/// lazily — the pool starts empty and allocates a fresh cache when a
/// thread checks out and the freelist is empty. Returned caches are
/// reused by the next checkout.
///
/// Construct with [`RaptorCachePool::for_timetable`] (or
/// [`RaptorCachePool::with_capacity`] when you don't have the timetable
/// in scope yet). Like [`RaptorCache`] itself, a pool is sized for one
/// specific timetable's `n_stops`/`n_routes` and panics on dimension
/// mismatch when its caches are used.
///
/// ```no_run
/// # use raptor::{RaptorCachePool, SecondOfDay, Timetable};
/// # fn ex<T: Timetable + Sync>(tt: &T, queries: &[(raptor::StopIdx, raptor::StopIdx)]) {
/// let pool = RaptorCachePool::for_timetable(tt);
/// // Sequential or parallel — same code:
/// for &(start, end) in queries {
///     let mut cache = pool.checkout();
///     let _ = tt.query()
///         .from(start)
///         .to(end)
///         .depart_at(SecondOfDay::hms(9, 0, 0))
///         .run_with_cache(&mut cache);
/// }
/// # }
/// ```
pub struct RaptorCachePool<L: Label = ArrivalTime> {
    n_stops: u32,
    n_routes: u32,
    pool: Mutex<Vec<RaptorCache<L>>>,
}

impl<L: Label> RaptorCachePool<L> {
    /// Constructs a pool sized for the given timetable. The pool starts
    /// empty; caches are allocated on first checkout.
    pub fn for_timetable<T: Timetable + ?Sized>(tt: &T) -> Self {
        Self::with_capacity(tt.n_stops() as u32, tt.n_routes() as u32)
    }

    /// Constructs a pool for a timetable with the given counts. Use
    /// [`for_timetable`](Self::for_timetable) when you have the timetable
    /// in scope.
    pub fn with_capacity(n_stops: u32, n_routes: u32) -> Self {
        Self {
            n_stops,
            n_routes,
            pool: Mutex::new(Vec::new()),
        }
    }

    /// Borrow a cache from the pool. Allocates a fresh one if the pool
    /// is empty. The returned guard returns the cache to the pool when
    /// dropped, ready for the next checkout.
    pub fn checkout(&self) -> PooledCache<'_, L> {
        let cache = self
            .pool
            .lock()
            .expect("RaptorCachePool mutex poisoned")
            .pop()
            .unwrap_or_else(|| RaptorCache::with_capacity(self.n_stops, self.n_routes));
        PooledCache {
            pool: self,
            cache: Some(cache),
        }
    }
}

impl<L: Label> std::fmt::Debug for RaptorCachePool<L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let len = self.pool.lock().map(|p| p.len()).unwrap_or(0);
        f.debug_struct("RaptorCachePool")
            .field("n_stops", &self.n_stops)
            .field("n_routes", &self.n_routes)
            .field("idle_caches", &len)
            .finish()
    }
}

/// RAII guard handed out by [`RaptorCachePool::checkout`]. Derefs to
/// [`RaptorCache`]; pass `&mut pooled_cache` wherever a `&mut RaptorCache`
/// is wanted. Returns the cache to the pool on drop.
pub struct PooledCache<'p, L: Label = ArrivalTime> {
    pool: &'p RaptorCachePool<L>,
    cache: Option<RaptorCache<L>>,
}

impl<L: Label> std::ops::Deref for PooledCache<'_, L> {
    type Target = RaptorCache<L>;
    fn deref(&self) -> &RaptorCache<L> {
        self.cache.as_ref().expect("PooledCache used after drop")
    }
}

impl<L: Label> std::ops::DerefMut for PooledCache<'_, L> {
    fn deref_mut(&mut self) -> &mut RaptorCache<L> {
        self.cache.as_mut().expect("PooledCache used after drop")
    }
}

impl<L: Label> Drop for PooledCache<'_, L> {
    fn drop(&mut self) {
        if let Some(cache) = self.cache.take() {
            // Best-effort return; if the mutex is poisoned, drop the cache.
            if let Ok(mut pool) = self.pool.pool.lock() {
                pool.push(cache);
            }
        }
    }
}
