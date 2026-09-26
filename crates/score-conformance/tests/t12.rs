use score_conformance::*;
use score_engine::*;

#[test]
fn t12_twinkle_under_every_scheduler() {
    let mut perfs = vec![];
    for sched in ["canonical", "random:1", "random:2", "by-timbre:piano,bass", "by-timbre:bass,piano"] {
        let s = load_file("t12_twinkle.score");
        let cfg = Config { scheduler: Scheduler::parse(sched).unwrap(), ..Default::default() };
        let mut e = engine(s, cfg, Sources::new(prng(0)));
        let (stop, evs) = run(&mut e, &Limits::default());
        assert_eq!(stop, Stop::Quiescent);
        // every written note passes the general name @0
        for (_, r) in evs.iter().flat_map(|ev| ev.records.iter()) {
            assert_eq!(r.payload, e.arena.nil());
            assert_eq!((r.ptimbre, r.carry), (score_core::Hat::Wild, score_core::Hat::Wild));
        }
        assert!(evs.iter().all(|ev| ev.alternatives == 1), "every fitting has one candidate");
        let p = e.sorted_performance();
        assert_eq!(p.len(), 22);
        let end = p.iter().map(|n| n.onset.add(&e.alph.len(n.dur))).max().unwrap();
        assert_eq!(end, score_core::Q::int(4));
        let table: Vec<(String, String, String, String)> = p
            .iter()
            .map(|n| (n.onset.to_string(), e.alph.timbre_name(n.timbre).to_string(), names(&e, n).0, names(&e, n).1))
            .collect();
        perfs.push(table);
    }
    for p in &perfs[1..] {
        assert_eq!(p, &perfs[0]);
    }
    // the chords of the phrase: I I IV I IV I V I
    let together: Vec<(String, String)> = {
        let p = &perfs[0];
        let mut v = vec![];
        for n in p.iter().filter(|n| n.1 == "bass") {
            let m = p.iter().find(|m| m.1 == "piano" && m.0 == n.0).unwrap();
            v.push((m.2.clone(), n.2.clone()));
        }
        v
    };
    assert_eq!(together.len(), 8);
    let bass: Vec<&str> = together.iter().map(|x| x.1.as_str()).collect();
    assert_eq!(bass, ["C3", "C3", "F3", "C3", "F3", "C3", "G3", "C3"]);
    assert_eq!(perfs[0][0], ("0".into(), "piano".into(), "C4".into(), "q".into()));
}
