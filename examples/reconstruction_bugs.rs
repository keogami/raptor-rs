//! Example demonstrating the "wrong boarding stop" bug in journey reconstruction
//!
//! ```text
//! ============================================================================
//!                        NETWORK DIAGRAM
//! ============================================================================
//!
//! ISSUE 1: Wrong boarding stop stored
//! ------------------------------------
//!
//!                    ┌─────── Route R1 (slow) ───────┐
//!                    │                               ▼
//!                    S                               A
//!                    │                               │
//!                    └─────── Route R2 (fast) ───┐   │
//!                                                ▼   │
//!                                                B◄──┘
//!                                                │
//!                                         Route R3 (shared)
//!                                                │
//!                                                ▼
//!                                                C
//!                                                │
//!                                                ▼
//!                                                D (target)
//!
//!   Route R1: S ──────────────────► A           (arrives A @ t=100)
//!   Route R2: S ──────────────────► B           (arrives B @ t=30)
//!   Route R3: A ────► B ────► C ────► D
//!
//!   Trips on R3:
//!     Trip T1 (late):  A(dep:105) → B(arr:110) → C(arr:120) → D(arr:130)
//!     Trip T2 (early): A(dep:25)  → B(arr:30)  → C(arr:40)  → D(arr:50)
//!
//!   Round 1: Reach A@100 (via R1), reach B@30 (via R2)
//!   Round 2: Scan R3 from A (earliest in route order)
//!            - At A: board T1 (earliest departing ≥100)
//!            - At B: T1 arrives@110, but we reached B@30! Board T2 instead.
//!            - At C: T2 arrives@40. Insert (2,C)→(A,R3) ← BUG! Boarded at B!
//!            - At D: T2 arrives@50. Insert (2,D)→(A,R3) ← BUG! Boarded at B!
//!
//!   Reconstruction says: S→A (R1), then A→D (R3)
//!   But actual journey:  S→B (R2), then B→D (R3)  ← faster!
//!
//! ============================================================================
//! ```

use raptor::{Tau, Timetable};

// =============================================================================
// ISSUE 1: Wrong boarding stop stored
// =============================================================================

/// Demonstrates the bug where the scan start position is stored
/// instead of the actual boarding stop.
///
/// Three routes:
/// - R1: S → A (slow, arrives at A @ t=100)
/// - R2: S → B (fast, arrives at B @ t=30)
/// - R3: A → B → C → D (two trips: T1 late, T2 early)
struct Issue1Timetable;

impl Timetable for Issue1Timetable {
    type Stop = char;
    type Route = &'static str;
    type Trip = u32;

    fn get_routes_serving_stop(&self, stop: Self::Stop) -> Vec<Self::Route> {
        match stop {
            'S' => vec!["R1", "R2"],
            'A' => vec!["R1", "R3"],
            'B' => vec!["R2", "R3"],
            'C' | 'D' => vec!["R3"],
            _ => vec![],
        }
    }

    fn get_earlier_stop(
        &self,
        route: Self::Route,
        left: Self::Stop,
        right: Self::Stop,
    ) -> Self::Stop {
        let order: &[char] = match route {
            "R1" => &['S', 'A'],
            "R2" => &['S', 'B'],
            "R3" => &['A', 'B', 'C', 'D'],
            _ => return left,
        };
        let l = order.iter().position(|&c| c == left).unwrap_or(99);
        let r = order.iter().position(|&c| c == right).unwrap_or(99);
        order[l.min(r)]
    }

    fn get_stops_after(&self, route: Self::Route, stop: Self::Stop) -> Vec<Self::Stop> {
        let order: &[char] = match route {
            "R1" => &['S', 'A'],
            "R2" => &['S', 'B'],
            "R3" => &['A', 'B', 'C', 'D'],
            _ => return vec![],
        };
        let pos = order.iter().position(|&c| c == stop).unwrap_or(0);
        order[pos..].to_vec()
    }

    // Trip IDs: 10 = R1, 20 = R2, 31 = R3/T1(late), 32 = R3/T2(early)
    fn get_earliest_trip(
        &self,
        route: Self::Route,
        at: Tau,
        stop: Self::Stop,
    ) -> Option<Self::Trip> {
        match route {
            // R1: single trip, departs S@0, arrives A@100
            "R1" => {
                let dep = match stop {
                    'S' => 0,
                    'A' => 100,
                    _ => return None,
                };
                (at <= dep).then_some(10)
            }
            // R2: single trip, departs S@0, arrives B@30
            "R2" => {
                let dep = match stop {
                    'S' => 0,
                    'B' => 30,
                    _ => return None,
                };
                (at <= dep).then_some(20)
            }
            // R3: two trips
            //   T1 (late):  A@105 → B@110 → C@120 → D@130
            //   T2 (early): A@25  → B@30  → C@40  → D@50
            "R3" => {
                let (t2_dep, t1_dep) = match stop {
                    'A' => (25, 105),
                    'B' => (30, 110),
                    'C' => (40, 120),
                    'D' => (50, 130),
                    _ => return None,
                };
                if at <= t2_dep {
                    Some(32) // T2 is earlier
                } else if at <= t1_dep {
                    Some(31) // T1 is later
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn get_arrival_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        match (trip, stop) {
            // R1 trip (10)
            (10, 'A') => 100,
            // R2 trip (20)
            (20, 'B') => 30,
            // R3 T1 (31, late trip)
            (31, 'B') => 110,
            (31, 'C') => 120,
            (31, 'D') => 130,
            // R3 T2 (32, early trip)
            (32, 'B') => 30,
            (32, 'C') => 40,
            (32, 'D') => 50,
            _ => Tau::MAX,
        }
    }

    fn get_departure_time(&self, trip: Self::Trip, stop: Self::Stop) -> Tau {
        match (trip, stop) {
            // R1 trip (10)
            (10, 'S') => 0,
            // R2 trip (20)
            (20, 'S') => 0,
            // R3 T1 (31, late trip)
            (31, 'A') => 105,
            (31, 'B') => 110,
            (31, 'C') => 120,
            // R3 T2 (32, early trip)
            (32, 'A') => 25,
            (32, 'B') => 30,
            (32, 'C') => 40,
            _ => Tau::MAX,
        }
    }

    fn get_footpaths_from(&self, _: Self::Stop) -> Vec<Self::Stop> {
        vec![]
    }
}

fn main() {
    println!("============================================================");
    println!("     RAPTOR Journey Reconstruction Bug Demonstration");
    println!("============================================================\n");

    println!("ISSUE 1: Wrong boarding stop stored");
    println!("------------------------------------");
    println!("Network:");
    println!("  R1: S ──────────────────► A      (arrives A @ t=100)");
    println!("  R2: S ──────────────────► B      (arrives B @ t=30)");
    println!("  R3: A ────► B ────► C ────► D");
    println!("      T1: A@105 → B@110 → C@120 → D@130  (late trip)");
    println!("      T2: A@25  → B@30  → C@40  → D@50   (early trip)");
    println!();
    println!("Query: S → D, departure time 0");
    println!();
    println!("Expected (optimal journey):");
    println!("  S ──(R2)──► B ──(R3/T2)──► D    arrives @ t=50");
    println!("  Reconstruction: [(R2, S), (R3, B)]");
    println!();
    println!("Bug produces (suboptimal, wrong boarding stop):");
    println!("  Reconstruction: [(R1, S), (R3, A)]");
    println!("  Because the algorithm stores A (scan start) instead of B (actual boarding)");
    println!();
    println!("Running RAPTOR...\n");

    let timetable = Issue1Timetable;
    let journey = timetable.raptor(3, 0, 'S', 'D');

    println!("{journey:#?}");
}
