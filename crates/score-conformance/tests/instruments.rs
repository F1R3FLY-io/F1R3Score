//! Revision 2: musicians and instruments (Section 9 of the note), records and
//! playback, collection with instruments, fans over a subset.
//!
//! T13 and T14 are the spec's conformance tests for the instrument layer;
//! the rest are its "additional tests".
use score_conformance::*;
use score_core::base::decode_keyloc;
use score_core::{Hat, Q};
use score_engine::*;
use score_render as render;
use std::collections::{BTreeMap, BTreeSet};

fn cfg(gc: bool) -> Config {
    Config { gc, ..Default::default() }
}

type Row = (String, String, String, String);

/// Run to the end and return the performance as (onset, timbre, pitch, dur)
/// rows, the number of null notes, and the engine.
fn play(e: &mut Engine) -> (Vec<Row>, usize) {
    let (stop, evs) = run(e, &Limits::default());
    assert_eq!(stop, Stop::Quiescent);
    let nulls = evs.iter().flat_map(|ev| ev.notes.iter()).filter(|n| !e.alph.len(n.dur).is_positive()).count();
    let rows = e
        .sorted_performance()
        .iter()
        .map(|n| (n.onset.to_string(), e.alph.timbre_name(n.timbre).to_string(), names(e, n).0, names(e, n).1))
        .collect();
    (rows, nulls)
}

fn rows(v: &[(&str, &str, &str)]) -> BTreeSet<(String, String, String)> {
    v.iter().map(|(t, p, d)| (t.to_string(), p.to_string(), d.to_string())).collect()
}
fn as_set(r: &[Row]) -> BTreeSet<(String, String, String)> {
    r.iter().map(|(t, _, p, d)| (t.clone(), p.clone(), d.clone())).collect()
}

#[test]
fn t13_the_four_hands_piano() {
    // two players on two keyboards: one performance under 20 random
    // schedules, exactly the table of Example 9.5, with 16 null notes
    let expected = rows(&[
        ("0", "A3", "q"),
        ("0", "A2", "h"),
        ("1/4", "C3", "q"),
        ("1/2", "E3", "q"),
        ("1/2", "E2", "h"),
        ("3/4", "A3", "h"),
        ("3/4", "C3", "h"),
        ("3/4", "E3", "h"),
    ]);
    for seed in 0..20 {
        let s = load_file("t13_fourhands.score");
        let c = Config { scheduler: Scheduler::Random(seed), ..Default::default() };
        let mut e = engine(s, c, Sources::new(prng(seed)));
        let (perf, nulls) = play(&mut e);
        assert_eq!(as_set(&perf), expected, "schedule {seed}");
        assert_eq!(perf.len(), 8);
        assert_eq!(nulls, 16, "two null notes per touch: an acknowledgement and a refresh");
    }
    // a third player on the first keyboard reaches for A3 at 0: exactly two
    // performances, the two the example describes
    let first_gets_it = rows(&[
        ("0", "A3", "q"),
        ("0", "A2", "h"),
        ("1/4", "A3", "q"),
        ("1/4", "C3", "q"),
        ("1/2", "E3", "q"),
        ("1/2", "E2", "h"),
        ("3/4", "A3", "h"),
        ("3/4", "C3", "h"),
        ("3/4", "E3", "h"),
    ]);
    let third_gets_it = rows(&[
        ("0", "A3", "q"),
        ("0", "A2", "h"),
        ("1/4", "A3", "q"),
        ("1/2", "C3", "q"),
        ("1/2", "E2", "h"),
        ("3/4", "E3", "q"),
        ("1", "A3", "h"),
        ("1", "C3", "h"),
        ("1", "E3", "h"),
    ]);
    let mut seen = BTreeSet::new();
    for seed in 0..20 {
        let s = load_file("t13_third.score");
        let c = Config { scheduler: Scheduler::Random(seed), ..Default::default() };
        let mut e = engine(s, c, Sources::new(prng(seed)));
        let (perf, nulls) = play(&mut e);
        assert_eq!(nulls, 18);
        seen.insert(as_set(&perf));
    }
    assert_eq!(seen.len(), 2, "exactly two distinct performances");
    assert!(seen.contains(&first_gets_it) && seen.contains(&third_gets_it));
}

/// A chimeric keyboard over {A3, C3, E3} with vibes and strings, and a
/// player who plays the A-minor arpeggio `reps` times in `dur`, either
/// leaving the timbre open or writing `timbre` into every touch.
fn chimera_src(reps: usize, dur: &str, timbre: &str) -> String {
    let mut s = String::from(
        "score Chimera\nimport std\npitches { r, C3, E3, A3 }\ndurations { e = 1/8, q = 1/4, h = 1/2 }\n\
         timbres { vibes = gm(11) on 1, strings = gm(48) on 2 }\n\
         factor vibes_temper = table dur { e: 9, q: 1, h: 1 }\n\
         factor strings_temper = table dur { e: 1, q: 1, h: 9 }\n\
         def P = base \"P\"\n\
         play ChimeraKeyboard(P, {A3, C3, E3}, [(vibes, vibes_temper), (strings, strings_temper)])\n   | ",
    );
    for _ in 0..reps {
        for k in ["A3", "C3", "E3"] {
            s.push_str(&format!("<@keyloc(P, {k}), {timbre}, {dur}>!(0) ; "));
        }
    }
    s.push_str("0\n");
    s
}

/// Play a chimera score step by step. Returns the vibes share of the notes,
/// the number of notes, the greatest number of receipts ever at one key
/// location, and whether each key location held exactly one receipt per
/// timbre whenever nothing there was sounding (Proposition 9.6).
fn run_chimera(src: &str, seed: u64) -> (f64, usize, usize, bool) {
    let s = load_src("chimera.score", src);
    let mut e = engine(s, cfg(false), Sources::new(prng(seed)));
    let (mut worst, mut whole) = (0usize, true);
    let mut sounding_until: BTreeMap<score_core::ProcId, Q> = BTreeMap::new();
    loop {
        match e.step(&Limits::default()).unwrap() {
            Err(_) => break,
            Ok(ev) => {
                for (t, r) in &ev.records {
                    let n = score_core::nu(r, &e.alph).unwrap();
                    if e.alph.len(n.dur).is_positive() {
                        sounding_until.insert(r.loc, t.add(&e.alph.len(n.dur)));
                    }
                }
                for (l, st) in &e.soup.locs {
                    if decode_keyloc(&e.arena, l.quote).is_none() {
                        continue;
                    }
                    worst = worst.max(st.recvs.len());
                    // a note ending exactly now is re-armed by a refresh at
                    // this same onset, which may not have fired yet
                    let busy = sounding_until.get(&l.quote).map_or(false, |u| u >= &ev.onset);
                    if !busy {
                        let ts: BTreeSet<_> = st.recvs.iter().map(|r| r.subjects[0].timbre).collect();
                        whole &= st.recvs.len() == 2 && ts.len() == 2;
                    }
                }
            }
        }
    }
    let perf = e.sorted_performance();
    let vibes = e.alph.lookup_timbre("vibes").unwrap();
    let share = perf.iter().filter(|n| n.timbre == vibes).count() as f64 / perf.len() as f64;
    (share, perf.len(), worst, whole)
}

#[test]
fn t14_the_chimeric_instrument() {
    // elaborating 4500 nested `;` recurses deeply: use a big stack
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(|| {
            for (dur, want, seed) in [("e", 0.9, 1), ("q", 0.5, 2), ("h", 0.1, 3)] {
                let (share, n, worst, whole) = run_chimera(&chimera_src(1500, dur, "_"), seed);
                eprintln!("T14 {dur} open: {n} notes, vibes share {share:.3}, at most {worst} receipts per key");
                assert_eq!(n, 4500);
                assert!((share - want).abs() <= 0.02, "{dur}: vibes share {share}");
                assert!(worst <= 2, "no key location ever holds more than two receipts");
                assert!(whole, "a chimera stays whole (Proposition 9.6)");
            }
            let (share, n, worst, _) = run_chimera(&chimera_src(1500, "h", "vibes"), 4);
            eprintln!("T14 h insisting on vibes: {n} notes, vibes share {share:.3}");
            assert_eq!(n, 4500);
            assert_eq!(share, 1.0, "a touch that names its timbre meets only that key");
            assert!(worst <= 2);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn keys_queue_and_synchronous_output_waits_for_the_longest_note() {
    for seed in 0..10 {
        let s = load_file("instruments.score");
        let c = Config { scheduler: Scheduler::Random(seed), ..Default::default() };
        let mut e = engine(s, c, Sources::new(prng(seed)));
        let mut worst_same_timbre = 0;
        let mut records = vec![];
        loop {
            match e.step(&Limits::default()).unwrap() {
                Err(_) => break,
                Ok(ev) => {
                    records.extend(ev.records.clone());
                    for (l, st) in &e.soup.locs {
                        if decode_keyloc(&e.arena, l.quote).is_some() {
                            let ts: Vec<_> = st.recvs.iter().map(|r| r.subjects[0].timbre).collect();
                            let distinct: BTreeSet<_> = ts.iter().collect();
                            worst_same_timbre = worst_same_timbre.max(ts.len() - distinct.len());
                        }
                    }
                }
            }
        }
        // Proposition 9.2: at most one receipt of each timbre at a key location
        assert_eq!(worst_same_timbre, 0);
        let perf = e.sorted_performance();
        let c4 = e.alph.lookup_datum("C4").unwrap();
        // the three touches aimed at C4 queue: their notes never overlap
        let mut spans: Vec<(Q, Q)> = perf.iter().filter(|n| n.pitch == c4).map(|n| (n.onset.clone(), n.onset.add(&e.alph.len(n.dur)))).collect();
        spans.sort();
        assert_eq!(spans.len(), 3);
        for w in spans.windows(2) {
            assert!(w[0].1 <= w[1].0, "touches at one key queue rather than overlap: {spans:?}");
        }
        // Proposition 9.4: the third player's step (E4 q | G4 h) releases its
        // C4 touch at max(1/4, 1/2) = 1/2, so the last C4 starts no earlier
        // than 1/2 (later if it queues behind another C4)
        assert!(spans.last().unwrap().0 >= Q::new(1, 2));
        let g4 = e.alph.lookup_datum("G4").unwrap();
        let g4_end = perf.iter().find(|n| n.pitch == g4).map(|n| n.onset.add(&e.alph.len(n.dur))).unwrap();
        assert_eq!(g4_end, Q::new(1, 2));
        // the acknowledgements: one per touch, each at the end of its note
        let ctl: Vec<Q> = records
            .iter()
            .filter(|(_, r)| r.recv.timbre == Hat::Is(score_core::Timbre::CTL))
            .map(|(t, _)| t.clone())
            .collect();
        assert_eq!(ctl.len(), 5, "five touches: 1 + 1 + (2) + 1");
        // two null notes per touch (Proposition 9.4): five touches
        let nulls = records.iter().filter(|(_, r)| !e.alph.len(score_core::nu(r, &e.alph).unwrap().dur).is_positive()).count();
        assert_eq!(nulls, 10);
    }
}

#[test]
fn every_touch_passes_the_general_name_and_every_note_is_nu_of_its_record() {
    let s = load_file("t13_fourhands.score");
    let mut e = engine(s, cfg(false), Sources::new(prng(0)));
    let (_, evs) = run(&mut e, &Limits::default());
    for ev in &evs {
        for ((t, r), n) in ev.records.iter().zip(ev.notes.iter()) {
            assert_eq!(&Note::of_record(t, r, &e.alph), n);
            if e.alph.len(n.dur).is_positive() {
                // a musical note: a touch (open timbre) meeting a key (piano)
                assert_eq!(r.send.timbre, Hat::Wild);
                assert_eq!(r.recv.timbre, Hat::Is(e.alph.lookup_timbre("piano").unwrap()));
            }
            // every payload here is a general name: touches, refreshes, acks
            assert_eq!((r.ptimbre, r.carry), (Hat::Wild, Hat::Wild));
        }
    }
}

/// Play a score, write its trace, and check that the notes the engine
/// reported are nu of the trace's records, and that `render` on the trace
/// gives the same performance and MIDI file as `play`.
fn records_and_render(file: &str, c: Config, bind: &[(&str, &str)], notes: Option<u64>) {
    let s = load_file(file);
    let digest = s.digest_hex();
    let mut src = Sources::new(prng(0));
    for (k, v) in bind {
        src.bind(k, score_chance::source(v, 64).unwrap());
    }
    let mut e = engine(s, c, src);
    let mut t = render::trace_header(&e, &digest, file) + "\n";
    let mut reported = vec![];
    e.run(&Limits { notes, ..Default::default() }, &mut |ev, eng| {
        for ((onset, r), n) in ev.records.iter().zip(ev.notes.iter()) {
            assert_eq!(&Note::of_record(onset, r, &eng.alph), n, "{file}: a note that is not nu of its record");
        }
        reported.extend(ev.notes.iter().cloned());
        t.push_str(&render::trace_event(ev, &eng.alph));
        t.push('\n');
    })
    .unwrap();
    let (alph, d2, rendered) = render::render_trace(&t).unwrap_or_else(|x| panic!("{file}: {x}"));
    assert_eq!(d2, digest);
    assert_eq!(alph, e.alph);
    assert_eq!(rendered, reported, "{file}: render reads the same notes off the records");
    let p1 = e.sorted_performance();
    let p2 = render::performance_of(&rendered, &alph);
    assert_eq!(p1, p2, "{file}");
    assert_eq!(render::midi::write(&p1, &e.alph, 120).bytes, render::midi::write(&p2, &alph, 120).bytes, "{file}: MIDI");
    assert_eq!(render::performance_json(&p1, &e.alph, &digest), render::performance_json(&p2, &alph, &digest));
}

#[test]
fn records_test_render_equals_play() {
    records_and_render("t13_fourhands.score", cfg(false), &[], None);
    records_and_render("t13_third.score", Config { scheduler: Scheduler::Random(3), ..Default::default() }, &[], None);
    records_and_render("instruments.score", cfg(false), &[], None);
    records_and_render("t12_twinkle.score", cfg(false), &[], None);
    records_and_render("t1_law.score", cfg(true), &[("A", "prng:7")], Some(300));
    records_and_render("t5_dual.score", cfg(true), &[("A", "prng:5")], Some(300));
    records_and_render("t4_embedding.score", cfg(true), &[("A.pitch", "spigot:pi/16@1"), ("A.dur", "spigot:e/5")], Some(200));
    records_and_render("t6_coupled.score", cfg(false), &[], None);
}

#[test]
fn collection_leaves_instruments_alone() {
    // FourHands run with --gc freshness collects nothing and plays as without
    // it (Finding "the simulator's collection test is unsound with
    // instruments"): key, code and acknowledgement locations are messages but
    // not record-shaped
    let mut perfs = vec![];
    for gc in [false, true] {
        let s = load_file("t13_fourhands.score");
        let mut e = engine(s, cfg(gc), Sources::new(prng(0)));
        let (perf, _) = play(&mut e);
        assert!(e.soup.tombstones.is_empty(), "nothing is collected");
        perfs.push(perf);
    }
    assert_eq!(perfs[0], perfs[1]);
    // a reflective voice's record locations are still collected
    let s = load_file("t1_law.score");
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(1));
    let mut e = engine(s, cfg(true), src);
    run(&mut e, &Limits { notes: Some(50), ..Default::default() });
    assert!(e.soup.tombstones.len() >= 49);
}

#[test]
fn fans_over_a_subset_equal_full_fans_with_a_crisp_exclusion() {
    let head = "import std\npitches { r, C4, D4, E4, F4, G4, A4 }\ndurations { q = 1/4, h = 1/2, e = 1/8 }\n\
                timbres { piano }\nfactor psi = machine pitch uniform * machine dur uniform\n";
    let subset = format!("{head}play VoiceOver(piano, psi, C4, q, \"A\", {{C4, E4, G4}}) #A\n");
    let full = format!("{head}play Voice(piano, psi * [carry(C4) || carry(E4) || carry(G4)], C4, q, \"A\") #A\n");
    let mut outs = vec![];
    for src in [&subset, &full] {
        let s = load_src("fan.score", src);
        let mut sources = Sources::new(prng(0));
        sources.bind("A", prng(21));
        let mut e = engine(s, cfg(true), sources);
        let (_, evs) = run(&mut e, &Limits { notes: Some(500), ..Default::default() });
        // record for record: onset, note and carried datum
        let recs: Vec<(String, String, String, String, String)> = evs
            .iter()
            .flat_map(|ev| ev.records.iter())
            .map(|(t, r)| {
                let n = score_core::nu(r, &e.alph).unwrap();
                let carry = match r.carry {
                    Hat::Is(d) => e.alph.name(d).to_string(),
                    Hat::Wild => "_".into(),
                };
                (t.to_string(), e.alph.timbre_name(n.timbre).into(), e.alph.name(n.pitch).into(), e.alph.name(n.dur).into(), carry)
            })
            .collect();
        outs.push(recs);
    }
    assert_eq!(outs[0].len(), outs[1].len());
    assert_eq!(outs[0], outs[1], "the subset fan is the full fan with the outside pitches weighted 0");
    assert!(outs[0].iter().all(|r| ["C4", "E4", "G4", "r"].contains(&r.2.as_str())));
}

#[test]
fn midi_golden_for_four_hands_and_chimera_routing() {
    let s = load_file("t13_fourhands.score");
    let mut e = engine(s, cfg(false), Sources::new(prng(0)));
    run(&mut e, &Limits::default());
    let m = render::midi::write(&e.sorted_performance(), &e.alph, 120);
    let golden = std::fs::read(scores_dir().join("../golden/fourhands.mid")).expect("golden file");
    assert_eq!(m.bytes, golden, "FourHands MIDI is byte-identical to the golden file");
    // a chimera run routes each note to its winning timbre's track
    let s = load_src("chimera.score", &chimera_src(30, "q", "_"));
    let mut e = engine(s, cfg(false), Sources::new(prng(8)));
    run(&mut e, &Limits::default());
    let perf = e.sorted_performance();
    let m = render::midi::write(&perf, &e.alph, 120);
    // count note-ons per track (tracks after the conductor, in timbre order)
    let mut per_track = vec![];
    let b = &m.bytes;
    let mut i = 14;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..i + 8 + len];
        per_track.push(body.windows(3).filter(|w| w[0] & 0xF0 == 0x90 && w[2] == 100).count());
        i += 8 + len;
    }
    let vibes = perf.iter().filter(|n| e.alph.timbre_name(n.timbre) == "vibes").count();
    let strings = perf.len() - vibes;
    assert!(vibes > 0 && strings > 0);
    assert_eq!(per_track[1..], [vibes, strings]);
}
