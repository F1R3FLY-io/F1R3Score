//! T0, T2, T3, T4, T6, T8, T9, T10, T11 and the additional tests.
use score_conformance::*;
use score_core::{Proc, Q};
use score_engine::*;

fn cfg(gc: bool) -> Config {
    Config { gc, ..Default::default() }
}

#[test]
fn t0_inertness_of_the_unplayed() {
    let s = load_file("t0_inertness.score");
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(1));
    let mut e = engine(s, cfg(false), src);
    let mut worst = 0;
    while e.played < 400 {
        let live = e.live_locations().unwrap();
        let records = live.iter().filter(|l| matches!(e.arena.get(l.quote), Proc::Send { .. })).count();
        worst = worst.max(records);
        e.step(&Limits::default()).unwrap().unwrap();
    }
    // messages left at record locations other than the current one
    let live = e.live_locations().unwrap();
    let stale: usize = e
        .soup
        .locs
        .iter()
        .filter(|(l, _)| matches!(e.arena.get(l.quote), Proc::Send { .. }) && !live.contains(l))
        .map(|(_, st)| st.sends.len())
        .sum();
    eprintln!("T0: max record locations with a candidate {worst}, stale messages {stale}");
    assert_eq!(worst, 1);
    assert_eq!(stale, 79 * 400, "exactly 5*16-1 unplayed messages per note");
}

#[test]
fn t11_reflective_voice_leaves_dead_hands_inert() {
    let s = load_file("t11_reflective_inert.score");
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(1));
    let mut e = engine(s, cfg(false), src);
    let (mut worst, mut worst_all) = (0, 0);
    while e.played < 200 {
        let live = e.live_locations().unwrap();
        worst = worst.max(live.iter().filter(|l| matches!(e.arena.get(l.quote), Proc::Send { .. })).count());
        worst_all = worst_all.max(live.len());
        e.step(&Limits::default()).unwrap().unwrap();
    }
    let live = e.live_locations().unwrap();
    let dead: usize = e
        .soup
        .locs
        .iter()
        .filter(|(l, _)| matches!(e.arena.get(l.quote), Proc::Send { .. }) && !live.contains(l))
        .map(|(_, st)| st.recvs.len())
        .sum();
    eprintln!("T11: record locations with a candidate {worst}, all {worst_all}, dead hands {dead}");
    assert_eq!(worst, 1);
    assert_eq!(dead, 15 * 200, "|Pit|-1 dead hands per note");
}

#[test]
fn t2_harmony_by_independence() {
    let mut perfs = vec![];
    for sched in ["canonical", "random:1", "random:2", "random:3", "by-timbre:piano"] {
        let s = load_file("t2_harmony.score");
        let mut src = Sources::new(prng(0));
        src.bind("A", prng(11));
        src.bind("B", prng(12));
        let c = Config { gc: true, scheduler: Scheduler::parse(sched).unwrap(), ..Default::default() };
        let mut e = engine(s, c, src);
        run(&mut e, &Limits { notes: Some(3000), ..Default::default() });
        // compare over a common horizon: the performance up to the least final onset
        perfs.push(e.sorted_performance());
    }
    let horizon = perfs.iter().map(|p| p.last().unwrap().onset.clone()).min().unwrap();
    let cut: Vec<Vec<Note>> = perfs.iter().map(|p| p.iter().filter(|n| n.onset < horizon).cloned().collect()).collect();
    for c in &cut[1..] {
        assert_eq!(c, &cut[0], "the performance does not depend on the schedule");
    }
    let mut by: std::collections::BTreeMap<Q, usize> = Default::default();
    for n in &cut[0] {
        *by.entry(n.onset.clone()).or_default() += 1;
    }
    let sims = by.values().filter(|v| **v >= 2).count();
    eprintln!("T2: {} notes, {sims} simultaneous strikes", cut[0].len());
    assert!(sims > 50);
}

#[test]
fn t3_two_sided_fitting() {
    for (mode, first) in [(Mode::Max, 3), (Mode::One, 1)] {
        for seed in 0..20 {
            let s = load_file("t3_two_sided.score");
            let c = Config { mode, ..Default::default() };
            let mut e = engine(s, c, Sources::new(prng(seed)));
            let (_, evs) = run(&mut e, &Limits::default());
            assert_eq!(evs[0].notes.len(), first, "{mode:?}: min(m,n) notes at once in max mode");
            let total: usize = evs.iter().map(|ev| ev.notes.len()).sum();
            assert_eq!(total, 3, "leftovers go on to emit min(m,n)-1 more in one mode");
        }
    }
}

#[test]
fn t4_embedding_of_the_spigot_instrument() {
    let s = load_file("t4_embedding.score");
    let mut src = Sources::new(prng(0));
    // the written first pitch is digit 0 of pi, so the pitch driver starts at 1
    src.bind("A.pitch", score_chance::source("spigot:pi/16@1", 64).unwrap());
    src.bind("A.dur", score_chance::source("spigot:e/5", 64).unwrap());
    let mut e = engine(s, cfg(true), src);
    let (_, evs) = run(&mut e, &Limits { notes: Some(400), ..Default::default() });
    let mut pi = score_chance::Spigot::new(score_chance::Constant::Pi, 16).unwrap();
    let mut ee = score_chance::Spigot::new(score_chance::Constant::E, 5).unwrap();
    use score_chance::DigitStream;
    let pd: Vec<u64> = (0..402).map(|_| pi.next_digit()).collect();
    let dd: Vec<u64> = (0..402).map(|_| ee.next_digit()).collect();
    let idx = indices(&e, &e.performance.clone());
    for (i, (p, d)) in idx.iter().enumerate() {
        assert_eq!(*p as u64, pd[i], "pitch {i} is the {i}-th hex digit of pi");
        assert_eq!(*d as u64, dd[i], "duration {i} is the {i}-th base-5 digit of e");
    }
    let musical: Vec<&Event> = evs.iter().filter(|ev| ev.alternatives > 1).collect();
    assert_eq!(musical.len(), 400);
    assert!(musical.iter().all(|ev| ev.routing == "factored" && ev.digits.iter().all(|x| x.1 == 1)));
}

#[test]
fn t6_simultaneity_by_contention() {
    let mut same = 0;
    for seed in 0..2000 {
        let s = load_file("t6_coupled.score");
        let mut e = engine(s, cfg(false), Sources::new(prng(seed)));
        let ev = e.step(&Limits::default()).unwrap().unwrap();
        assert_eq!(ev.notes.len(), 2, "one resolution strikes both hands");
        if ev.notes[0].dur == ev.notes[1].dur {
            same += 1;
        }
    }
    assert_eq!(same, 0, "the durations are never equal (independent draws: 1/5)");
}

fn run_voice(file: &str, notes: u64, seed: u64) -> Vec<usize> {
    let s = load_file(file);
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(seed));
    let mut e = engine(s, cfg(true), src);
    run(&mut e, &Limits { notes: Some(notes), ..Default::default() });
    let p = e.performance.clone();
    indices(&e, &p).iter().map(|x| x.0).collect()
}

#[test]
fn t8_idioms_read_from_the_past() {
    let count = |ps: &[usize]| {
        let triples = ps.windows(3).filter(|w| w[0] == w[1] && w[1] == w[2]).count();
        let mot = ps.windows(3).filter(|w| (w[0], w[1], w[2]) == (0, 2, 4)).count();
        let ctx = ps.windows(3).filter(|w| (w[0], w[1]) == (0, 2)).count();
        (triples, mot as f64 / ctx.max(1) as f64, ctx)
    };
    let (t_u, m_u, _) = count(&run_voice("t8_unconstrained.score", 10_000, 5));
    let (t_f, _, _) = count(&run_voice("t8_forbid.score", 10_000, 5));
    let (_, m_m, c_m) = count(&run_voice("t8_motif.score", 10_000, 5));
    eprintln!("T8: triples unconstrained {t_u}, forbidden {t_f}; motif completion {m_u:.3} -> {m_m:.3} ({c_m} contexts)");
    assert!(t_u > 0);
    assert_eq!(t_f, 0);
    assert!(m_m > m_u + 0.2);
}

#[test]
fn t9_table_equals_its_formula() {
    use score_core::{Datum, Hat, Timbre};
    use score_logic::*;
    let s = load_file("t1_law.score");
    let mut a = s.arena;
    let alph = s.alph;
    let mut tables: Vec<Table> = (0..s.clauses.len() as u32)
        .filter_map(|i| match s.clauses.get(score_core::ClauseId(i)) {
            Graded::Table(t) => Some(t.clone()),
            Graded::Tensor(v) => v.iter().find_map(|g| if let Graded::Table(t) = g { Some(t.clone()) } else { None }),
            _ => None,
        })
        .collect();
    assert!(tables.len() >= 2);
    let np = alph.n_pitches() as u64;
    let nd = 5;
    // a `dur`-keyed table, as a chimera's articulation clause (Example 9.7)
    let mut entries = std::collections::BTreeMap::new();
    for (i, w) in [(0u64, 9), (1, 1), (2, 1)] {
        entries.insert([KeyVal::D(Datum((np + i) as u16)), KeyVal::Pad, KeyVal::Pad], score_core::Q::int(w));
    }
    tables.push(Table { key: vec![KeyFn::Dur(0)], entries, default: score_core::Q::new(1, 2) });
    let mut rng = score_chance::Prng::new(2);
    let base = score_core::base::base(&mut a, "x", alph.rest());
    let t0 = Hat::Is(Timbre(0));
    let mut worst = 0usize;
    for _ in 0..3000 {
        let d = |i: u64| Datum((np + i) as u16);
        let p = |i: u64| Datum(i as u16);
        let h = a.send(a.quote(base, t0, Hat::Is(d(rng.below(nd)))), base, t0, Hat::Is(p(rng.below(np))));
        // the carried datum is sometimes open: tables keyed by it are then
        // undefined there, and so is their formula
        let carry = if rng.below(6) == 0 { Hat::Wild } else { Hat::Is(p(rng.below(np))) };
        let slots = [SlotView {
            timbre: Timbre(0),
            pitch: p(rng.below(np)),
            dur: d(rng.below(nd)),
            carry,
            ptimbre: Hat::Wild,
            passes: base,
        }];
        let view = View { timbre: Timbre(0), loc: h, slots: &slots };
        let mut ev = Evaluator::new(&a, &alph);
        for t in &tables {
            let x = ev.table(t, &view);
            let y = ev.graded(&t.to_formula(), &view, &mut NoBehaviour);
            if x != y {
                worst += 1;
            }
        }
    }
    assert_eq!(worst, 0);
}

#[test]
fn t10_reflective_voice_plays_the_unrolled_voice() {
    // elaborating 600 nested steps recurses deeply: use a big stack
    std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| {
            let mut perfs = vec![];
            let mut nulls = 0;
            for f in ["t10_reflective.score", "t10_unrolled.score"] {
                let s = load_file(f);
                let mut src = Sources::new(prng(4));
                src.bind("A", prng(99));
                let mut e = engine(s, cfg(true), src);
                let (_, evs) = run(&mut e, &Limits { notes: Some(590), ..Default::default() });
                if f.contains("reflective") {
                    // every request and every refresh delivered to the server
                    // passes a general name: both payload decorations `_`
                    let null_records: Vec<_> = evs
                        .iter()
                        .flat_map(|ev| ev.records.iter())
                        .filter(|(_, r)| !e.alph.len(score_core::nu(r, &e.alph).unwrap().dur).is_positive())
                        .collect();
                    assert!(!null_records.is_empty());
                    assert!(null_records
                        .iter()
                        .all(|(_, r)| r.ptimbre == score_core::Hat::Wild && r.carry == score_core::Hat::Wild));
                    let zero: Vec<&Note> = evs
                        .iter()
                        .flat_map(|ev| ev.notes.iter())
                        .filter(|n| !e.alph.len(n.dur).is_positive())
                        .collect();
                    assert!(zero.iter().all(|n| n.pitch == e.alph.rest() && n.dur == e.alph.eps()));
                    nulls = zero.len();
                }
                perfs.push(e.performance.clone());
            }
            eprintln!("T10: {} notes, {nulls} null notes", perfs[0].len());
            assert_eq!(perfs[0], perfs[1], "identical performance");
            assert!((2 * 590 - 1..=2 * 590 + 1).contains(&nulls), "two null notes per note");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn freshness_guard() {
    let src = r#"
pitches { r, C4 } durations { q = 1/4 } timbres { piano }
play for (_ <- <@(<@0, piano, q>!(0, piano, C4)), piano, C4>) { <@base "X", piano, q>!(0, piano, r) }
   | <@(<@0, piano, q>!(0, piano, C4)), piano, q>!(0, piano, r)
   | for (_ <- <@base "X", piano, C4>) {
       for (_ <- <@(<@0, piano, q>!(0, piano, C4)), piano, C4>) { 0 }
       | <@(<@0, piano, q>!(0, piano, C4)), piano, q>!(0, piano, r) }
"#;
    let s = load_src("fresh.score", src);
    let mut e = engine(s, cfg(true), Sources::new(prng(0)));
    let err = e.run(&Limits::default(), &mut |_, _| {}).err().expect("must fail");
    assert_eq!(err.kind, "freshness-violated");
    // without collection it plays to the end
    let s = load_src("fresh.score", src);
    let mut e = engine(s, cfg(false), Sources::new(prng(0)));
    let (stop, _) = run(&mut e, &Limits::default());
    assert_eq!(stop, Stop::Quiescent);
    assert_eq!(e.performance.len(), 3);
}

#[test]
fn chords_strike_together_and_wait_for_the_longest() {
    let src = r#"
import std
pitches { r, C4, E4, G4 } durations { q = 1/4, h = 1/2 } timbres { piano }
play for (a <- <@base "K", piano, C4> & b <- <@base "K", piano, E4> & c <- <@base "K", piano, G4>
          where [carry#1(G4)]) { N(base "after", piano, C4, q; 0) }
   | <@base "K", piano, q>!(0, piano, G4) | <@base "K", piano, h>!(0, piano, G4)
   | <@base "K", piano, q>!(0, piano, E4)
"#;
    for seed in 0..10 {
        let s = load_src("chord.score", src);
        let mut e = engine(s, cfg(false), Sources::new(prng(seed)));
        let (_, evs) = run(&mut e, &Limits::default());
        assert_eq!(evs[0].notes.len(), 3, "a chord emits its three notes at one onset");
        assert!(evs[0].notes.iter().all(|n| n.onset == Q::ZERO));
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[1].onset, Q::new(1, 2), "the continuation waits for the longest duration");
    }
}

#[test]
fn enforcement_without_deadlock() {
    let triples = |e: &Engine| {
        let ps: Vec<usize> = indices(e, &e.performance).iter().map(|x| x.0).collect();
        ps.windows(3).filter(|w| w[0] == w[1] && w[1] == w[2]).count()
    };
    // without the viability factor the voice falls silent
    let mut silent = 0;
    for seed in 0..5 {
        let s = load_file("viable_absent.score");
        let mut src = Sources::new(prng(0));
        src.bind("A", prng(seed));
        let mut e = engine(s, cfg(true), src);
        let (stop, _) = run(&mut e, &Limits { notes: Some(10_000), ..Default::default() });
        if stop == Stop::Quiescent {
            silent += 1;
        }
        assert_eq!(triples(&e), 0);
    }
    assert_eq!(silent, 5, "the rule alone deadlocks");
    let t = std::time::Instant::now();
    let s = load_file("viable.score");
    assert!(s.behavioural);
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(1));
    let mut e = engine(s, cfg(true), src);
    let (stop, _) = run(&mut e, &Limits { notes: Some(10_000), ..Default::default() });
    eprintln!("viable: {:?} after {} notes in {:?}, {} explored states", stop, e.played, t.elapsed(), e.explorer_states());
    assert_eq!(stop, Stop::Notes, "never falls silent");
    assert_eq!(triples(&e), 0, "never violates the rule");
    let ps: Vec<usize> = indices(&e, &e.performance).iter().map(|x| x.0).collect();
    assert!(!ps.contains(&2), "never enters the trap");
}
