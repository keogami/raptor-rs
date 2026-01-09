# RAPTOR Implementation Soundness Analysis

This document analyzes the soundness of the RAPTOR implementation in `src/lib.rs` against the original paper:

> Delling, D., Pajor, T., & Werneck, R. F. (2012). *Round-Based Public Transit Routing*. ALENEX.

---

## RAPTOR Algorithm Overview

The RAPTOR algorithm is a Round-bAsed Public Transit Optimized Router that computes all Pareto-optimal journeys minimizing **arrival time** and **number of transfers**.

### Core Data Structures

- **Timetable**: (Π, S, T, R, F) where:
  - Π = period of operation (e.g., seconds of a day)
  - S = set of stops
  - T = set of trips
  - R = set of routes (trips sharing the same sequence of stops)
  - F = set of transfers/footpaths

- **Multilabel**: Each stop p has (τ₀(p), τ₁(p), ..., τₖ(p)) where τᵢ(p) is the earliest arrival with at most i trips

### Algorithm Structure

**Initialization:**
- Set all τᵢ(p) = ∞
- Set τ₀(pₛ) = τ (departure time from source)
- Mark source stop pₛ

**Per Round k (computing journeys with k trips / k-1 transfers):**

1. **Stage 1**: Set τₖ(p) = τₖ₋₁(p) for all stops (upper bound from previous round)

2. **Stage 2**: Traverse routes
   - Collect routes Q serving marked stops
   - For each route r starting at earliest marked stop p:
     - Find earliest catchable trip t
     - Traverse stops, update τₖ(pⱼ) with arrival times
     - Switch to earlier trip if τₖ₋₁(pᵢ) < τₐᵣᵣ(t, pᵢ)

3. **Stage 3**: Process footpaths
   - For each (pᵢ, pⱼ) ∈ F: τₖ(pⱼ) = min{τₖ(pⱼ), τₖ(pᵢ) + ℓ(pᵢ, pⱼ)}

**Optimizations:**
- **Marking**: Only traverse routes containing stops improved in the previous round
- **Local pruning**: Track τ*(pᵢ) = earliest arrival at pᵢ across all rounds
- **Target pruning**: Don't mark stops with arrival > τ*(pₜ)

---

## Soundness Issues

### Issue 1: `get_earliest_trip` Missing Time Parameter

**Severity**: Critical

**Location**: `src/lib.rs:26` (trait definition) and `src/lib.rs:94` (usage)

**Paper definition**:
```
et(r, pᵢ) = earliest trip t in route r such that τ_dep(t, pᵢ) ≥ τₖ₋₁(pᵢ)
```

The function must know the arrival time at the stop to determine which trips are catchable.

**Current signature**:
```rust
fn get_earliest_trip(&self, route: Self::Route, stop: Self::Stop) -> Option<Self::Trip>;
```

**Problem**: Missing the time parameter `τₖ₋₁(pᵢ)`. The function cannot determine which trip is "earliest catchable" without knowing when we arrive at that stop.

**Expected signature**:
```rust
fn get_earliest_trip(&self, route: Self::Route, stop: Self::Stop, min_departure: Tau) -> Option<Self::Trip>;
```

---

### Issue 2: Footpath Transfer Time Not Added

**Severity**: Critical

**Location**: `src/lib.rs:105-109`

**Paper (Algorithm 1, line 26)**:
```
τₖ(p') ← min{τₖ(p'), τₖ(p) + ℓ(p, p')}
```

**Current code**:
```rust
let tau = *best_arrival_per_k
    .get(&(k, p_dash))
    .unwrap_or(&Tau::MAX)
    .min(best_arrival_per_k.get(&(k, stop)).unwrap_or(&Tau::MAX));
```

**Problem**: The transfer/walking time `ℓ(p, p')` is NOT added. The code takes the minimum of arrival times without accounting for walking duration. This effectively treats footpaths as instantaneous teleportation.

**Expected code**:
```rust
let transfer_time = self.get_transfer_time(stop, p_dash);
let arrival_at_stop = *best_arrival_per_k.get(&(k, stop)).unwrap_or(&Tau::MAX);
let via_footpath = arrival_at_stop.saturating_add(transfer_time);
let current_best = *best_arrival_per_k.get(&(k, p_dash)).unwrap_or(&Tau::MAX);
let tau = current_best.min(via_footpath);
```

---

### Issue 3: `get_stops_after` May Skip Boarding Stop

**Severity**: High

**Location**: `src/lib.rs:71`

**Paper**: "foreach stop pᵢ of r **beginning with p**"

The traversal should start FROM the marked stop p (inclusive), as this is where we potentially board the trip.

**Current code**:
```rust
for mut pi in self.get_stops_after(route, p)
```

**Problem**: The semantics of `get_stops_after` are ambiguous. If it returns stops strictly after p (excluding p), the algorithm will never attempt to board at the marked stop itself.

**Resolution**: Either:
1. Ensure `get_stops_after` includes the starting stop, or
2. Rename to `get_stops_from` with inclusive semantics, or
3. Handle the boarding stop separately before the loop

---

### Issue 4: Loop Range Excludes Final Round

**Severity**: Medium

**Location**: `src/lib.rs:49`

**Current code**:
```rust
for k in 1..transfers
```

**Problem**: Rust's `..` range is exclusive of the upper bound. If user passes `transfers=3`, only rounds 1 and 2 execute. The algorithm will miss journeys requiring exactly `transfers-1` transfers.

**Expected code**:
```rust
for k in 1..=transfers
```

---

### Issue 5: `best_arrival` (τ*) Not Updated in Footpath Stage

**Severity**: Medium

**Location**: `src/lib.rs:101-114`

**Paper**: When improving arrival times via footpaths, τ*(p') should also be updated for proper local pruning in subsequent rounds.

**Problem**: The footpath stage only updates `best_arrival_per_k` but not `best_arrival` (which represents τ*). This means:
1. Local pruning in future rounds may be less effective
2. The algorithm may do redundant work

**Expected behavior**: After updating `best_arrival_per_k.insert((k, p_dash), tau)`, also update:
```rust
let current_best = best_arrival.get(&p_dash).unwrap_or(&Tau::MAX);
if tau < *current_best {
    best_arrival.insert(p_dash, tau);
}
```

---

### Issue 6: Trip Update Logic Depends on Broken `get_earliest_trip`

**Severity**: Medium

**Location**: `src/lib.rs:87-95`

**Current code**:
```rust
if dbg!(*best_arrival_per_k.get(&(k - 1, pi)).unwrap_or(&Tau::MAX))
    <= dbg!(
        current_trip
            .map(|trip| self.get_departure_time(trip, pi))
            .unwrap_or(Tau::MAX)
    )
{
    current_trip = self.get_earliest_trip(route, pi);
}
```

**Problem**: When `current_trip` is `None`, the departure time defaults to `MAX`. This means `arrival <= MAX` is always true whenever we've reached `pi` in round k-1, triggering a call to `get_earliest_trip`.

However, since `get_earliest_trip` lacks the time parameter (Issue 1), it cannot correctly find the earliest trip departing after our arrival time. The two issues compound each other.

---

### Issue 7: Missing Target Pruning in Footpath Stage

**Severity**: Low

**Location**: `src/lib.rs:110`

**Paper (Algorithm 1, lines 18-21)**: Stops should only be marked if arrival time improves over both τ*(pᵢ) and τ*(pₜ).

**Problem**: The footpath stage marks all reachable stops unconditionally:
```rust
more_marked_stops.push(p_dash);
```

**Expected behavior**: Only mark if the new arrival time is better than the current best to target:
```rust
if tau < *best_arrival.get(&pt).unwrap_or(&Tau::MAX) {
    more_marked_stops.push(p_dash);
}
```

---

## Summary Table

| Issue | Severity | Location | Description |
|-------|----------|----------|-------------|
| 1 | **Critical** | Line 26, 94 | `get_earliest_trip` missing time parameter |
| 2 | **Critical** | Line 105-109 | Footpath transfer time not added |
| 3 | **High** | Line 71 | Potential off-by-one in stop traversal |
| 4 | **Medium** | Line 49 | Loop range excludes final round |
| 5 | **Medium** | Line 101-114 | `best_arrival` not updated for footpaths |
| 6 | **Medium** | Line 87-95 | Trip update logic depends on broken `get_earliest_trip` |
| 7 | **Low** | Line 110 | Missing target pruning in footpath stage |

---

## Impact Assessment

The two **critical** bugs (Issues 1 and 2) will cause the algorithm to produce incorrect results:

1. **Without the time parameter in `get_earliest_trip`**: The algorithm cannot correctly identify which trips are catchable from a given stop. It may select trips that have already departed or miss earlier available trips.

2. **Without adding transfer times in footpaths**: Walking connections are treated as instantaneous. This will produce journeys that appear faster than physically possible and may cause the algorithm to prefer infeasible routes.

The **high** severity issue (Issue 3) may cause the algorithm to miss valid boarding opportunities at marked stops.

The **medium** severity issues affect completeness and efficiency but may still produce some valid (if incomplete) results.
