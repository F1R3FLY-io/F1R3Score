//! The example scores run and behave as their comments say.
use score_conformance::*;
use score_engine::*;

fn load_example(name: &str) -> score_syntax::Score {
    let dir = scores_dir().join("../../../examples");
    let src = std::fs::read_to_string(dir.join(name)).unwrap();
    score_syntax::load(name, &src, &mut FsLoader(dir)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn call_and_response_hands_over_the_history() {
    for seed in 0..5 {
        let s = load_example("call_response.score");
        let mut src = Sources::new(prng(0));
        src.bind("call", prng(seed));
        src.bind("resp", prng(seed + 100));
        let mut e = engine(s, Config { gc: true, ..Default::default() }, src);
        run(&mut e, &Limits { notes: Some(6), ..Default::default() });
        let p = e.sorted_performance();
        let v: Vec<(String, String)> = p.iter().map(|n| (e.alph.timbre_name(n.timbre).to_string(), names(&e, n).0)).collect();
        assert_eq!(v[..5].iter().map(|x| x.1.as_str()).collect::<Vec<_>>(), ["C4", "E4", "G4", "G4", "D5"]);
        assert_eq!(v[3].0, "cello", "the line changes hands at the responder's first note");
    }
}

#[test]
fn hocket_alternates_players_on_one_line() {
    let s = load_example("hocket.score");
    let mut e = engine(s, Config { gc: true, ..Default::default() }, Sources::new(prng(5)));
    run(&mut e, &Limits { notes: Some(400), ..Default::default() });
    let ps: Vec<String> = e.performance.iter().map(|n| names(&e, n).0).collect();
    // odd notes are player B's (never C4/D4 chosen by A for B... ) -- check the
    // constraint each player imposes on the note it hands to the other
    for (i, p) in ps.iter().enumerate().skip(1) {
        if i % 2 == 1 {
            assert!(p != "C5" && p != "B4", "A never hands B the top: {i} {p}");
        } else {
            assert!(p != "C4" && p != "D4", "B never hands A the bottom: {i} {p}");
        }
    }
    // one line: every interval is a step of at most two degrees
    let ord = |p: &str| e.alph.ord(e.alph.lookup_datum(p).unwrap()).unwrap();
    assert!(ps.windows(2).all(|w| (ord(&w[1]) - ord(&w[0])).abs() <= 2));
}

#[test]
fn country_voice_plays_two_independent_voices() {
    let s = load_example("country_voice.score");
    assert!(!s.behavioural);
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(3));
    src.bind("B", prng(4));
    let mut e = engine(s, Config { gc: true, ..Default::default() }, src);
    run(&mut e, &Limits { notes: Some(200), ..Default::default() });
    let timbres: std::collections::BTreeSet<_> = e.performance.iter().map(|n| n.timbre).collect();
    assert_eq!(timbres.len(), 2);
}
