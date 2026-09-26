//! Replay, trace and MIDI tests.
use score_conformance::*;
use score_engine::*;
use score_render as render;

fn traced(file: &str, cfg: Config, bind: &[(&str, &str)], notes: u64) -> (String, Vec<Note>, String) {
    let s = load_file(file);
    let digest = s.digest_hex();
    let mut src = Sources::new(prng(0));
    for (k, v) in bind {
        src.bind(k, score_chance::source(v, 64).unwrap());
    }
    let mut e = engine(s, cfg, src);
    let mut t = render::trace_header(&e, &digest, file) + "\n";
    e.run(&Limits { notes: Some(notes), ..Default::default() }, &mut |ev, eng| {
        t.push_str(&render::trace_event(ev, &eng.alph));
        t.push('\n');
    })
    .unwrap();
    (t, e.sorted_performance(), digest)
}

fn replay(file: &str, trace: &str, cfg: Config) -> Result<Vec<Note>, EngineError> {
    let s = load_file(file);
    let tr = render::parse_trace(trace, &s.alph).unwrap();
    assert_eq!(tr.digest, s.digest_hex());
    // the chance sources are irrelevant: every choice comes from the trace
    let mut e = engine(s, cfg, Sources::new(prng(12345)));
    e.set_replay(tr.recorded);
    e.run(&Limits { max_steps: tr.last_step.map(|x| x + 1), ..Default::default() }, &mut |_, _| {})?;
    Ok(e.sorted_performance())
}

#[test]
fn every_run_replays_to_an_identical_performance() {
    let runs: Vec<(&str, Config, Vec<(&str, &str)>)> = vec![
        ("t1_law.score", Config { gc: true, ..Default::default() }, vec![("A", "prng:7")]),
        ("t2_harmony.score", Config { gc: true, scheduler: Scheduler::Random(3), ..Default::default() }, vec![("A", "prng:1"), ("B", "prng:2")]),
        ("t3_two_sided.score", Config::default(), vec![]),
        ("t4_embedding.score", Config { gc: true, ..Default::default() }, vec![("A.pitch", "spigot:pi/16@1"), ("A.dur", "spigot:e/5")]),
        ("t5_dual.score", Config { gc: true, ..Default::default() }, vec![("A", "prng:5")]),
        ("t6_coupled.score", Config::default(), vec![]),
        ("t8_forbid.score", Config { gc: true, ..Default::default() }, vec![("A", "prng:5")]),
        ("t12_twinkle.score", Config::default(), vec![]),
        ("viable.score", Config { gc: true, ..Default::default() }, vec![("A", "prng:3")]),
    ];
    for (f, cfg, bind) in runs {
        let (t, perf, _) = traced(f, cfg.clone(), &bind, 300);
        let again = replay(f, &t, cfg.clone()).unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(perf, again, "{f}: replay differs");
        // a tampered choice is detected
        if let Some(pos) = t.find("\"routing\":\"joint\"") {
            let start = t[..pos].rfind("\"chosen\":").unwrap() + 9;
            let end = start + t[start..].find(',').unwrap();
            let old: usize = t[start..end].parse().unwrap();
            let bad = format!("{}{}{}", &t[..start], if old == 0 { 1 } else { 0 }, &t[end..]);
            let r = replay(f, &bad, cfg);
            assert!(r.map(|p| p != perf).unwrap_or(true), "{f}: tampering undetected");
        }
    }
}

#[test]
fn replay_rejects_another_score() {
    let (t, _, _) = traced("t1_law.score", Config { gc: true, ..Default::default() }, &[("A", "prng:7")], 20);
    let s = load_file("t5_dual.score");
    let tr = render::parse_trace(&t, &s.alph).unwrap();
    assert_ne!(tr.digest, s.digest_hex());
}

#[test]
fn twinkle_midi_is_byte_identical_to_the_golden_file() {
    let s = load_file("t12_twinkle.score");
    let mut e = engine(s, Config::default(), Sources::new(prng(0)));
    run(&mut e, &Limits::default());
    let m = render::midi::write(&e.sorted_performance(), &e.alph, 120);
    assert_eq!(m.division, 1, "quarters and halves need one tick per quarter");
    assert!(m.warning.is_none());
    let golden = scores_dir().join("../golden/twinkle.mid");
    if std::env::var("F1R3SCORE_BLESS").is_ok() {
        std::fs::write(&golden, &m.bytes).unwrap();
    }
    let g = std::fs::read(&golden).expect("golden file (bless with F1R3SCORE_BLESS=1)");
    assert_eq!(m.bytes, g);
}

#[test]
fn midi_division_is_the_least_common_tick() {
    let src = r#"
import std
pitches { r, C4 } durations { t = 1/6, q = 1/4 } timbres { piano = gm(0) on 1 }
play line(base "A", piano) [ C4 t, C4 t, C4 t ] | line(base "B", piano) [ C4 q, C4 q ]
"#;
    let s = load_src("x.score", src);
    let mut e = engine(s, Config::default(), Sources::new(prng(0)));
    run(&mut e, &Limits::default());
    let m = render::midi::write(&e.sorted_performance(), &e.alph, 120);
    assert_eq!(m.division, 3, "triplet eighths against quarters: 3 ticks per quarter");
}
