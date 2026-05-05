use crate::Duration;
use crate::RaptorCache;
use crate::Tau;
use crate::Timetable;
use crate::simple::SimpleTimetable;

macro_rules! plan {
    ($tt:expr; $(($route:expr, $stop:expr)),* $(,)?) => {
        vec![$(($tt.route_idx_of(&$route), $tt.stop_idx_of(&$stop))),*]
    };
}

/// When a faster route reaches a mid-route stop, the algorithm must record
/// that stop as the boarding stop — not the earlier stop where the route
/// scan began. See examples/reboarding.rs for the full network.
#[test]
fn reboarding_picks_correct_boarding_stop() {
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        S,
        A,
        B,
        C,
        D,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
        R3,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        R1T1,
        R2T1,
        R3Late,
        R3Early,
    }

    let tt = SimpleTimetable::new()
        .route(
            R1,
            &[S, A],
            &[(R1T1, &[(Tau(0), Tau(0)), (Tau(100), Tau(100))])],
        )
        .route(
            R2,
            &[S, B],
            &[(R2T1, &[(Tau(0), Tau(0)), (Tau(30), Tau(30))])],
        )
        .route(
            R3,
            &[A, B, C, D],
            &[
                (
                    R3Late,
                    &[
                        (Tau(105), Tau(105)),
                        (Tau(110), Tau(110)),
                        (Tau(120), Tau(120)),
                        (Tau(130), Tau(130)),
                    ],
                ),
                (
                    R3Early,
                    &[
                        (Tau(25), Tau(25)),
                        (Tau(30), Tau(30)),
                        (Tau(40), Tau(40)),
                        (Tau(50), Tau(50)),
                    ],
                ),
            ],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&S), Duration::ZERO)],
        &[(tt.stop_idx_of(&D), Duration::ZERO)],
    );

    // The optimal journey: S->B via R2, then B->D via R3/Early, arriving at t=50
    assert!(!journeys.is_empty(), "should find at least one journey");

    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    assert_eq!(best.arrival(), Tau(50));
    assert_eq!(best.plan, plan!(tt; (R2, B), (R3, D)));
}

// ── Edge case tests ─────────────────────────────────────────────────

#[test]
fn no_journey_disconnected_graph() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::D), Duration::ZERO)],
    );
    assert!(
        journeys.is_empty(),
        "disconnected graph should yield no journeys"
    );
}

#[test]
fn no_journey_missed_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(50), Tau(50))])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(Tau(0), Tau(30)), (Tau(40), Tau(40))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::C), Duration::ZERO)],
    );
    assert!(
        journeys.is_empty(),
        "missed connection should yield no journeys"
    );
}

#[test]
fn no_journey_late_departure() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(Tau(0), Tau(10)), (Tau(20), Tau(20))])],
    );

    let journeys = tt.raptor(
        3,
        Tau(100),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    assert!(
        journeys.is_empty(),
        "late departure should yield no journeys"
    );
}

#[test]
fn no_journey_transfers_zero() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
    );

    let journeys = tt.raptor(
        0,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    assert!(journeys.is_empty(), "transfers=0 should yield no journeys");
}

#[test]
fn source_equals_target() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
    );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
    );
    assert!(
        journeys.is_empty(),
        "source == target should yield no journeys"
    );
}

#[test]
fn direct_journey_single_route() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B, Stop::C],
        &[(
            Trip::T1,
            &[(Tau(0), Tau(0)), (Tau(10), Tau(10)), (Tau(20), Tau(20))],
        )],
    );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::C), Duration::ZERO)],
    );
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].arrival(), Tau(20));
    assert_eq!(journeys[0].plan, plan!(tt; (Route::R1, Stop::C)));
}

#[test]
fn direct_journey_picks_fastest_route() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(100), Tau(100))])],
        )
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(Tau(0), Tau(0)), (Tau(50), Tau(50))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    assert_eq!(best.arrival(), Tau(50));
}

#[test]
fn exact_time_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(20), Tau(20))])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(Tau(0), Tau(20)), (Tau(30), Tau(30))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::C), Duration::ZERO)],
    );
    assert!(!journeys.is_empty(), "exact-time connection should work");
    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    assert_eq!(best.arrival(), Tau(30));
    assert_eq!(
        best.plan,
        plan!(tt; (Route::R1, Stop::B), (Route::R2, Stop::C))
    );
}

#[test]
fn multi_trip_picks_earliest_catchable() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[
            (Trip::T1, &[(Tau(0), Tau(5)), (Tau(15), Tau(15))]),
            (Trip::T2, &[(Tau(0), Tau(15)), (Tau(25), Tau(25))]),
            (Trip::T3, &[(Tau(0), Tau(25)), (Tau(35), Tau(35))]),
        ],
    );

    // Query at tau=12: T1 departs A@5 (too early), T2 departs A@15 (catchable)
    let journeys = tt.raptor(
        3,
        Tau(12),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].arrival(), Tau(25)); // T2 arrives B@25
}

#[test]
fn two_transfer_journey() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
        R3,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(Tau(0), Tau(10)), (Tau(20), Tau(20))])],
        )
        .route(
            Route::R3,
            &[Stop::C, Stop::D],
            &[(Trip::T3, &[(Tau(0), Tau(20)), (Tau(30), Tau(30))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::D), Duration::ZERO)],
    );
    assert!(!journeys.is_empty());
    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    assert_eq!(best.arrival(), Tau(30));
    assert_eq!(
        best.plan,
        plan!(tt;
            (Route::R1, Stop::B),
            (Route::R2, Stop::C),
            (Route::R3, Stop::D),
        )
    );
}

#[test]
fn pareto_optimal_fewer_transfers_vs_faster() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
        R3,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
        T3,
    }

    let tt = SimpleTimetable::new()
        // Direct slow route A→D
        .route(
            Route::R1,
            &[Stop::A, Stop::D],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(200), Tau(200))])],
        )
        // Fast 2-leg: A→B via R2, B→D via R3
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(Tau(0), Tau(0)), (Tau(40), Tau(40))])],
        )
        .route(
            Route::R3,
            &[Stop::B, Stop::D],
            &[(Trip::T3, &[(Tau(0), Tau(40)), (Tau(100), Tau(100))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::D), Duration::ZERO)],
    );
    assert_eq!(journeys.len(), 2, "should have 2 pareto-optimal journeys");

    let mut sorted = journeys.clone();
    sorted.sort_by_key(|j| j.arrival());
    // Faster journey (2 legs)
    assert_eq!(sorted[0].arrival(), Tau(100));
    assert_eq!(sorted[0].plan.len(), 2);
    // Slower direct journey (1 leg)
    assert_eq!(sorted[1].arrival(), Tau(200));
    assert_eq!(sorted[1].plan.len(), 1);
}

#[test]
fn footpath_enables_connection() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(Tau(0), Tau(20)), (Tau(30), Tau(30))])],
        )
        .footpath(Stop::B, Stop::C)
        .transfer_time(Stop::B, Stop::C, Duration(5));

    // Optimal: board R1 at A, alight at B (arr 10), walk B→C (arr 15),
    // board R2 at C (dep 20), alight at D (arr 30). Two boardings, with a
    // walk leg between them. Reconstruction traces back through the walk.
    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::D), Duration::ZERO)],
    );
    assert_eq!(journeys.len(), 1, "expected one journey, got {journeys:?}");
    assert_eq!(journeys[0].arrival(), Tau(30));
    assert_eq!(
        journeys[0].plan,
        plan!(tt; (Route::R1, Stop::B), (Route::R2, Stop::D))
    );
}

#[test]
fn footpath_transfer_time_causes_miss() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(50), Tau(50))])],
        )
        .route(
            Route::R2,
            &[Stop::C, Stop::D],
            &[(Trip::T2, &[(Tau(0), Tau(52)), (Tau(60), Tau(60))])],
        )
        .footpath(Stop::B, Stop::C)
        .transfer_time(Stop::B, Stop::C, Duration(5)); // 50+5=55 > 52

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::D), Duration::ZERO)],
    );
    assert!(
        journeys.is_empty(),
        "footpath transfer time should cause miss"
    );
}

#[test]
fn early_termination_no_improvement() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        Route::R1,
        &[Stop::A, Stop::B],
        &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
    );

    let j1 = tt.raptor(
        1,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    let j100 = tt.raptor(
        100,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );

    assert_eq!(j1.len(), j100.len());
    assert_eq!(
        j1.iter().min_by_key(|j| j.arrival()).unwrap().arrival(),
        j100.iter().min_by_key(|j| j.arrival()).unwrap().arrival(),
    );
}

#[test]
fn dominance_prunes_slower_arrival() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(50), Tau(50))])],
        )
        .route(
            Route::R2,
            &[Stop::A, Stop::B],
            &[(Trip::T2, &[(Tau(0), Tau(0)), (Tau(100), Tau(100))])],
        );

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(tt.stop_idx_of(&Stop::A), Duration::ZERO)],
        &[(tt.stop_idx_of(&Stop::B), Duration::ZERO)],
    );
    // Both routes are discovered in round 1, so the slower one is dominated
    assert_eq!(journeys.len(), 1, "dominated journey should be pruned");
    assert_eq!(journeys[0].arrival(), Tau(50));
}

#[test]
fn raptor_with_cache_matches_fresh_run() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::B],
            &[(Trip::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            Route::R2,
            &[Stop::B, Stop::C],
            &[(Trip::T2, &[(Tau(15), Tau(15)), (Tau(25), Tau(25))])],
        );

    // Run several queries with varying parameters through one cache and
    // confirm each result matches a fresh-run baseline.
    let queries = [
        (3, Tau(0), Stop::A, Stop::C),
        (1, Tau(0), Stop::A, Stop::B),
        (5, Tau(5), Stop::A, Stop::C),
        (2, Tau(0), Stop::B, Stop::C),
    ];

    let mut cache: RaptorCache = RaptorCache::for_timetable(&tt);
    for &(transfers, tau, ps, pt) in &queries {
        let ps_idx = tt.stop_idx_of(&ps);
        let pt_idx = tt.stop_idx_of(&pt);
        let baseline = tt.raptor(
            transfers,
            tau,
            &[(ps_idx, Duration::ZERO)],
            &[(pt_idx, Duration::ZERO)],
        );
        let cached = tt.raptor_with_cache(
            &mut cache,
            transfers,
            tau,
            &[(ps_idx, Duration::ZERO)],
            &[(pt_idx, Duration::ZERO)],
        );
        assert_eq!(
            cached.len(),
            baseline.len(),
            "journey count differs at query {ps:?}->{pt:?}"
        );
        for (b, c) in baseline.iter().zip(cached.iter()) {
            assert_eq!(b.arrival(), c.arrival());
            assert_eq!(b.plan, c.plan);
        }
    }
}

#[test]
fn multi_source_picks_best_origin() {
    // Two parallel single-trip routes. R_fast goes A->C in 10. R_slow goes
    // B->C in 30 (departs same time but the trip itself takes longer).
    // Query "from {A, B} to C" should pick the journey via A (faster).
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        RFast,
        RSlow,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        TFast,
        TSlow,
    }

    let tt = SimpleTimetable::new()
        .route(
            RFast,
            &[A, C],
            &[(TFast, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            RSlow,
            &[B, C],
            &[(TSlow, &[(Tau(0), Tau(0)), (Tau(30), Tau(30))])],
        );

    let a = tt.stop_idx_of(&A);
    let b = tt.stop_idx_of(&B);
    let c = tt.stop_idx_of(&C);

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(a, Duration::ZERO), (b, Duration::ZERO)],
        &[(c, Duration::ZERO)],
    );
    assert_eq!(journeys.len(), 1, "one Pareto-optimal journey expected");
    assert_eq!(journeys[0].arrival(), Tau(10));
    assert_eq!(
        journeys[0].origin, a,
        "should have started at A (the faster route)"
    );
    assert_eq!(journeys[0].target, c);
    assert_eq!(journeys[0].plan, plan!(tt; (RFast, C)));
}

#[test]
fn multi_source_walk_offset_changes_best_origin() {
    // Same routes as above, but the user has a 30s walk to A and 0s to B.
    // The fast trip via A now effectively takes 30 + 10 = 40s of departure
    // delay + travel, while the slow trip via B takes 0 + 30 = 30s. So B
    // wins.
    use Route::*;
    use Stop::*;
    use Trip::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        RFast,
        RSlow,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        TFast,
        TSlow,
    }

    let tt = SimpleTimetable::new()
        // The slow trip needs to depart at 30 (so it leaves later than the
        // user is ready) for the algorithm to actually pick it cleanly.
        .route(
            RFast,
            &[A, C],
            &[(TFast, &[(Tau(30), Tau(30)), (Tau(40), Tau(40))])],
        )
        .route(
            RSlow,
            &[B, C],
            &[(TSlow, &[(Tau(0), Tau(0)), (Tau(30), Tau(30))])],
        );

    let a = tt.stop_idx_of(&A);
    let b = tt.stop_idx_of(&B);
    let c = tt.stop_idx_of(&C);

    // Walk 30s to A means the user reaches A at tau=30. By then TFast is
    // about to depart; arrival at C = 40.
    // Walk 0s to B means the user reaches B at tau=0. TSlow boards at 0,
    // arrives C at 30.
    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(a, Duration(30)), (b, Duration::ZERO)],
        &[(c, Duration::ZERO)],
    );
    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    assert_eq!(best.arrival(), Tau(30));
    assert_eq!(
        best.origin, b,
        "should have started at B (closer + slow trip wins)"
    );
}

#[test]
fn multi_target_walk_offset_picks_best_target() {
    // Two targets, T1 reachable at 10 with walk 30 (effective 40), T2
    // reachable at 25 with walk 0 (effective 25). T2 should win.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Stop {
        A,
        T1,
        T2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Route {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Trip {
        Tr1,
        Tr2,
    }

    let tt = SimpleTimetable::new()
        .route(
            Route::R1,
            &[Stop::A, Stop::T1],
            &[(Trip::Tr1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            Route::R2,
            &[Stop::A, Stop::T2],
            &[(Trip::Tr2, &[(Tau(0), Tau(0)), (Tau(25), Tau(25))])],
        );

    let a = tt.stop_idx_of(&Stop::A);
    let t1 = tt.stop_idx_of(&Stop::T1);
    let t2 = tt.stop_idx_of(&Stop::T2);

    let journeys = tt.raptor(
        3,
        Tau(0),
        &[(a, Duration::ZERO)],
        &[(t1, Duration(30)), (t2, Duration::ZERO)],
    );
    let best = journeys.iter().min_by_key(|j| j.arrival()).unwrap();
    // best raw arrival at T1 = 10, +30 walk = 40 effective
    // best raw arrival at T2 = 25, +0 walk = 25 effective → wins
    assert_eq!(best.arrival(), Tau(25));
    assert_eq!(best.target, t2);
}

#[test]
fn query_builder_single_departure() {
    use crate::labels::ArrivalAndWalk;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        R::R1,
        &[S::A, S::B, S::C],
        &[(
            Tr::T1,
            &[(Tau(0), Tau(0)), (Tau(10), Tau(10)), (Tau(20), Tau(20))],
        )],
    );
    let a = tt.stop_idx_of(&S::A);
    let c = tt.stop_idx_of(&S::C);

    // 1. Bare typestate flow with all defaults except from/to/depart_at.
    let journeys = tt.query().from(a).to(c).depart_at(Tau(0)).run();
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].arrival(), Tau(20));

    // 2. With max_transfers and a u8 literal (Into<Transfers>).
    let journeys = tt
        .query()
        .from(a)
        .to(c)
        .max_transfers(3u8)
        .depart_at(Tau(0))
        .run();
    assert_eq!(journeys.len(), 1);

    // 3. With explicit cache (run_with_cache).
    let mut cache = crate::RaptorCache::for_timetable(&tt);
    let journeys = tt
        .query()
        .from(a)
        .to(c)
        .depart_at(Tau(0))
        .run_with_cache(&mut cache);
    assert_eq!(journeys.len(), 1);

    // 4. Custom label via query_with_label.
    let journeys: Vec<crate::Journey<ArrivalAndWalk>> = tt
        .query_with_label::<ArrivalAndWalk>()
        .from(a)
        .to(c)
        .depart_at(Tau(0))
        .run();
    assert_eq!(journeys.len(), 1);
    assert_eq!(journeys[0].label.walk_time, Duration::ZERO);
}

#[test]
fn query_builder_range_departure() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
        T3,
    }

    let tt = SimpleTimetable::new().route(
        R::R1,
        &[S::A, S::B],
        &[
            (Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))]),
            (Tr::T2, &[(Tau(10), Tau(10)), (Tau(20), Tau(20))]),
            (Tr::T3, &[(Tau(20), Tau(20)), (Tau(30), Tau(30))]),
        ],
    );
    let a = tt.stop_idx_of(&S::A);
    let b = tt.stop_idx_of(&S::B);

    let profile = tt
        .query()
        .from(a)
        .to(b)
        .depart_in_window([Tau(0), Tau(5), Tau(10), Tau(15), Tau(20)])
        .run();

    let mut points: Vec<(Tau, Tau)> = profile
        .iter()
        .map(|p| (p.depart, p.journey.arrival()))
        .collect();
    points.sort();
    assert_eq!(
        points,
        vec![(Tau(0), Tau(10)), (Tau(10), Tau(20)), (Tau(20), Tau(30))]
    );
}

#[test]
fn into_endpoints_accepts_natural_input_shapes() {
    // The single-stop call is the headline ergonomics win — `start` and
    // `end` go straight in, no slice-of-tuples wrapping.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
    }

    let tt = SimpleTimetable::new().route(
        R::R1,
        &[S::A, S::B, S::C],
        &[(
            Tr::T1,
            &[(Tau(0), Tau(0)), (Tau(10), Tau(10)), (Tau(20), Tau(20))],
        )],
    );
    let a = tt.stop_idx_of(&S::A);
    let b = tt.stop_idx_of(&S::B);
    let c = tt.stop_idx_of(&S::C);

    // 1. Bare StopIdx — the trivial case.
    let j = tt.raptor(3, Tau(0), a, c);
    assert_eq!(j.len(), 1);
    assert_eq!(j[0].arrival(), Tau(20));

    // 2. (StopIdx, Duration) pair as a single endpoint with explicit walk.
    //    Walk-time offset of 0 should match the bare-stop behaviour above.
    let j = tt.raptor(3, Tau(0), (a, Duration::ZERO), c);
    assert_eq!(j[0].arrival(), Tau(20));

    // 3. Slice of stops (multi-source / multi-target with walk = 0).
    let stops = [a, b];
    let j = tt.raptor(3, Tau(0), &stops[..], c);
    assert_eq!(j[0].arrival(), Tau(20));

    // 4. Slice of (stop, duration) pairs — original v0.13 shape.
    let pairs = [(a, Duration::ZERO)];
    let j = tt.raptor(3, Tau(0), &pairs[..], &[(c, Duration::ZERO)][..]);
    assert_eq!(j[0].arrival(), Tau(20));

    // 5. Owned Vec of (stop, duration) pairs.
    let owned = vec![(a, Duration::ZERO)];
    let j = tt.raptor(3, Tau(0), &owned, c);
    assert_eq!(j[0].arrival(), Tau(20));

    // 6. Pre-built Endpoints.
    let mut ep = crate::Endpoints::new();
    ep.push(a, Duration::ZERO);
    let j = tt.raptor(3, Tau(0), ep, c);
    assert_eq!(j[0].arrival(), Tau(20));
}

#[test]
fn closed_path_dispatch_matches_dijkstra() {
    use crate::{RouteIdx, StopIdx, Tau, TripIdx};

    // Newtype wrapper that delegates everything to its inner timetable
    // but reports the footpath relation as transitively closed. Run the
    // same scenario through both paths and assert identical journeys.
    struct ClosedAssert<T: Timetable>(T);

    impl<T: Timetable> Timetable for ClosedAssert<T> {
        fn n_stops(&self) -> usize {
            self.0.n_stops()
        }
        fn n_routes(&self) -> usize {
            self.0.n_routes()
        }
        fn get_routes_serving_stop(&self, stop: StopIdx) -> &[(RouteIdx, u32)] {
            self.0.get_routes_serving_stop(stop)
        }
        fn get_stops_after(&self, route: RouteIdx, pos: u32) -> &[StopIdx] {
            self.0.get_stops_after(route, pos)
        }
        fn stop_at(&self, route: RouteIdx, pos: u32) -> StopIdx {
            self.0.stop_at(route, pos)
        }
        fn get_earliest_trip(&self, route: RouteIdx, at: Tau, pos: u32) -> Option<TripIdx> {
            self.0.get_earliest_trip(route, at, pos)
        }
        fn get_arrival_time(&self, trip: TripIdx, pos: u32) -> Tau {
            self.0.get_arrival_time(trip, pos)
        }
        fn get_departure_time(&self, trip: TripIdx, pos: u32) -> Tau {
            self.0.get_departure_time(trip, pos)
        }
        fn get_footpaths_from(&self, stop: StopIdx) -> &[StopIdx] {
            self.0.get_footpaths_from(stop)
        }
        fn get_transfer_time(&self, from: StopIdx, to: StopIdx) -> Duration {
            self.0.get_transfer_time(from, to)
        }
        fn footpaths_are_transitively_closed(&self) -> bool {
            true
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
    }

    // A→B by R1, walk B→C, C→D by R2. The walk is a single direct edge
    // so closure is trivially satisfied — both paths must agree.
    let inner = SimpleTimetable::new()
        .route(
            R::R1,
            &[S::A, S::B],
            &[(Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            R::R2,
            &[S::C, S::D],
            &[(Tr::T2, &[(Tau(0), Tau(15)), (Tau(25), Tau(25))])],
        )
        .footpath(S::B, S::C)
        .transfer_time(S::B, S::C, Duration(3));

    let a = inner.stop_idx_of(&S::A);
    let d = inner.stop_idx_of(&S::D);

    let dijkstra = inner.raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(d, Duration::ZERO)]);
    let closed =
        ClosedAssert(inner).raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(d, Duration::ZERO)]);

    assert_eq!(dijkstra.len(), closed.len(), "journey count must match");
    for (a, b) in dijkstra.iter().zip(closed.iter()) {
        assert_eq!(a.arrival(), b.arrival(), "arrival must match");
        assert_eq!(a.plan, b.plan, "plan must match");
    }
}

#[test]
fn arrival_and_walk_label_tracks_accumulated_walk_time() {
    use crate::Duration;
    use crate::RaptorCache;
    use crate::labels::ArrivalAndWalk;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
    }

    // A→B by R1 (arr 10), walk B→C 7s, then C→D... actually just B→C:
    // we use a R2 from C to test. Walk B→C is 7s; final journey uses
    // 7s of walking.
    let tt = SimpleTimetable::new()
        .route(
            R::R1,
            &[S::A, S::B],
            &[(Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(R::R2, &[S::C], &[(Tr::T2, &[(Tau(20), Tau(20))])])
        .footpath(S::B, S::C)
        .transfer_time(S::B, S::C, Duration(7));

    let a = tt.stop_idx_of(&S::A);
    let c = tt.stop_idx_of(&S::C);

    // Default ArrivalTime path
    let default_journeys = tt.raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(c, Duration::ZERO)]);
    assert!(!default_journeys.is_empty());
    assert_eq!(default_journeys[0].arrival(), Tau(17)); // 10 + 7

    // Public ArrivalAndWalk via raptor_with_label.
    let label_journeys: Vec<crate::Journey<ArrivalAndWalk>> = tt
        .raptor_with_label::<ArrivalAndWalk>(
            3,
            Tau(0),
            &[(a, Duration::ZERO)],
            &[(c, Duration::ZERO)],
        );
    assert_eq!(label_journeys.len(), default_journeys.len());
    assert_eq!(label_journeys[0].arrival(), Tau(17));
    assert_eq!(
        label_journeys[0].label.walk_time,
        Duration(7),
        "walk time tracked"
    );

    // Same via raptor_with_cache_and_label (cache infers L).
    let mut cache = RaptorCache::<ArrivalAndWalk>::for_timetable(&tt);
    let cached = tt.raptor_with_cache_and_label(
        &mut cache,
        3,
        Tau(0),
        &[(a, Duration::ZERO)],
        &[(c, Duration::ZERO)],
    );
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].label.walk_time, Duration(7));
}

#[test]
fn raptor_range_returns_pareto_profile_across_departures() {
    // R1 has three departures: T1 (0->10), T2 (10->20), T3 (20->30).
    // Querying departures [0, 5, 10, 15, 20]:
    // - depart=0  catches T1, arr=10
    // - depart=5  catches T2, arr=20 (no choice but to wait)
    // - depart=10 catches T2, arr=20 — strictly better than depart=5
    //   (later departure, same arrival) → depart=5 dominated, dropped.
    // - depart=15 catches T3, arr=30 (must wait)
    // - depart=20 catches T3, arr=30 — better than depart=15, drop it.
    // Profile: [(0, arr 10), (10, arr 20), (20, arr 30)].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
        T3,
    }

    let tt = SimpleTimetable::new().route(
        R::R1,
        &[S::A, S::B],
        &[
            (Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))]),
            (Tr::T2, &[(Tau(10), Tau(10)), (Tau(20), Tau(20))]),
            (Tr::T3, &[(Tau(20), Tau(20)), (Tau(30), Tau(30))]),
        ],
    );

    let a = tt.stop_idx_of(&S::A);
    let b = tt.stop_idx_of(&S::B);

    let profile = tt.raptor_range(
        3,
        [Tau(0), Tau(5), Tau(10), Tau(15), Tau(20)],
        &[(a, Duration::ZERO)],
        &[(b, Duration::ZERO)],
    );

    let mut points: Vec<(crate::Tau, crate::Tau)> = profile
        .iter()
        .map(|p| (p.depart, p.journey.arrival()))
        .collect();
    points.sort();

    assert_eq!(
        points,
        vec![(Tau(0), Tau(10)), (Tau(10), Tau(20)), (Tau(20), Tau(30))]
    );
}

#[test]
fn arrival_and_walk_returns_pareto_front() {
    // Two routes reach two intermediate stops; both stops walk to the
    // target with different walk times. Path via X is faster but with
    // more walking; path via Y is slower but with less walking. Neither
    // dominates the other on (arrival, walk_time), so an
    // `ArrivalAndWalk` query should return both. An `ArrivalTime`
    // query returns only the arrival-min path.
    use crate::labels::ArrivalAndWalk;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        X,
        Y,
        T,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
    }

    // R1 arrives X at t=10; walk X->T is 5s   → (arr 15, walk 5).
    // R2 arrives Y at t=20; walk Y->T is 1s   → (arr 21, walk 1).
    let tt = SimpleTimetable::new()
        .route(
            R::R1,
            &[S::A, S::X],
            &[(Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            R::R2,
            &[S::A, S::Y],
            &[(Tr::T2, &[(Tau(0), Tau(0)), (Tau(20), Tau(20))])],
        )
        .footpath(S::X, S::T)
        .transfer_time(S::X, S::T, Duration(5))
        .footpath(S::Y, S::T)
        .transfer_time(S::Y, S::T, Duration(1));

    let a = tt.stop_idx_of(&S::A);
    let t = tt.stop_idx_of(&S::T);

    // ArrivalTime: only the arrival-min path survives.
    let arrival_only = tt.raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(t, Duration::ZERO)]);
    assert_eq!(arrival_only.len(), 1);
    assert_eq!(arrival_only[0].arrival(), Tau(15));

    // ArrivalAndWalk: both Pareto-incomparable journeys are returned.
    let pareto: Vec<crate::Journey<ArrivalAndWalk>> = tt.raptor_with_label::<ArrivalAndWalk>(
        3,
        Tau(0),
        &[(a, Duration::ZERO)],
        &[(t, Duration::ZERO)],
    );
    assert_eq!(
        pareto.len(),
        2,
        "expected Pareto front of two journeys, got {}: {:?}",
        pareto.len(),
        pareto.iter().map(|j| j.label).collect::<Vec<_>>(),
    );
    let mut labels: Vec<_> = pareto.iter().map(|j| j.label).collect();
    labels.sort_by_key(|l| l.arrival);
    assert_eq!(labels[0].arrival, Tau(15));
    assert_eq!(labels[0].walk_time, Duration(5));
    assert_eq!(labels[1].arrival, Tau(21));
    assert_eq!(labels[1].walk_time, Duration(1));
}

#[test]
fn with_timing_recovers_per_leg_trip_and_times() {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1Early,
        T1Late,
        T2,
    }

    // R1 has two trips: T1Early (0->10) and T1Late (50->60). R2 has one
    // trip from B to C (12->25). A query departing at 0 should ride
    // T1Early (boarding 0, alighting 10), then T2 (boarding 12, alighting
    // 25). Departing at 30 should ride T1Late (boarding 50, alighting 60)
    // — there's no T2 trip available afterwards, so no journey.
    let tt = SimpleTimetable::new()
        .route(
            R::R1,
            &[S::A, S::B],
            &[
                (Tr::T1Early, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))]),
                (Tr::T1Late, &[(Tau(50), Tau(50)), (Tau(60), Tau(60))]),
            ],
        )
        .route(
            R::R2,
            &[S::B, S::C],
            &[(Tr::T2, &[(Tau(12), Tau(12)), (Tau(25), Tau(25))])],
        );

    let a = tt.stop_idx_of(&S::A);
    let b = tt.stop_idx_of(&S::B);
    let c = tt.stop_idx_of(&S::C);

    let journeys = tt.raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(c, Duration::ZERO)]);
    assert_eq!(journeys.len(), 1);
    let j = &journeys[0];
    assert_eq!(j.arrival(), Tau(25));

    let timed = j
        .with_timing(&tt, Tau(0), Duration::ZERO)
        .expect("plan must reconstruct");
    assert_eq!(timed.len(), 2);

    // Leg 1: A -> B on R1 catching T1Early.
    let l1 = &timed[0];
    assert_eq!(l1.route, tt.route_idx_of(&R::R1));
    assert_eq!(l1.board, a);
    assert_eq!(l1.alight, b);
    assert_eq!(l1.trip, tt.trip_idx_of(&Tr::T1Early));
    assert_eq!(l1.depart, Tau(0));
    assert_eq!(l1.arrive, Tau(10));

    // Leg 2: B -> C on R2 catching T2.
    let l2 = &timed[1];
    assert_eq!(l2.route, tt.route_idx_of(&R::R2));
    assert_eq!(l2.board, b);
    assert_eq!(l2.alight, c);
    assert_eq!(l2.trip, tt.trip_idx_of(&Tr::T2));
    assert_eq!(l2.depart, Tau(12));
    assert_eq!(l2.arrive, Tau(25));
}

#[test]
fn with_timing_handles_one_hop_walking_transfer() {
    // Journey: ride R1 from A to B (arrive 10), walk B->C (5s), ride R2
    // from C to D (depart 20, arrive 30). The plan is [(R1, B), (R2, D)];
    // with_timing should detect that C is a one-hop walk neighbour of B
    // serving R2 and use C as leg 2's `board`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum S {
        A,
        B,
        C,
        D,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum R {
        R1,
        R2,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    enum Tr {
        T1,
        T2,
    }

    let tt = SimpleTimetable::new()
        .route(
            R::R1,
            &[S::A, S::B],
            &[(Tr::T1, &[(Tau(0), Tau(0)), (Tau(10), Tau(10))])],
        )
        .route(
            R::R2,
            &[S::C, S::D],
            &[(Tr::T2, &[(Tau(20), Tau(20)), (Tau(30), Tau(30))])],
        )
        .footpath(S::B, S::C)
        .transfer_time(S::B, S::C, Duration(5));

    let a = tt.stop_idx_of(&S::A);
    let b = tt.stop_idx_of(&S::B);
    let c = tt.stop_idx_of(&S::C);
    let d = tt.stop_idx_of(&S::D);

    let journeys = tt.raptor(3, Tau(0), &[(a, Duration::ZERO)], &[(d, Duration::ZERO)]);
    assert_eq!(journeys.len(), 1);
    let j = &journeys[0];

    let timed = j
        .with_timing(&tt, Tau(0), Duration::ZERO)
        .expect("plan must reconstruct");
    assert_eq!(timed.len(), 2);

    let l1 = &timed[0];
    assert_eq!(l1.alight, b, "leg 1 alights at B");

    let l2 = &timed[1];
    assert_eq!(l2.board, c, "leg 2 boards at C, not B (walking transfer)");
    assert_eq!(l2.alight, d);
    assert_eq!(l2.depart, Tau(20));
    assert_eq!(l2.arrive, Tau(30));
}
