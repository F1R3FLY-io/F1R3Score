//! T1, T5, T7: the law of a voice, its dual, and style (x) physicality.
use score_conformance::*;
use score_engine::*;

fn play(file: &str, notes: u64, seed: u64) -> (Engine, Vec<Note>) {
    let s = load_file(file);
    let cfg = Config { gc: true, ..Default::default() };
    let mut src = Sources::new(prng(0));
    src.bind("A", prng(seed));
    let mut e = engine(s, cfg, src);
    let (_, _) = run(&mut e, &Limits { notes: Some(notes), ..Default::default() });
    let p = e.performance.clone();
    (e, p)
}

#[test]
fn t1_law_of_one_voice() {
    let t = std::time::Instant::now();
    let (e, p) = play("t1_law.score", 20_000, 7);
    eprintln!("T1: 20000 notes in {:?}", t.elapsed());
    let m = machines("t1_law.machines");
    let (pp, pd) = (rownorm(&m["pitch"]), rownorm(&m["dur"]));
    let idx = indices(&e, &p);
    let ps: Vec<usize> = idx.iter().map(|x| x.0).collect();
    let ds: Vec<usize> = idx.iter().map(|x| x.1).collect();
    let (zp, op) = law_z(&ps, &pp, 16, 200);
    let (zd, od) = law_z(&ds, &pd, 5, 200);
    eprintln!("T1 pitch zmax {zp:.2} off {op}; dur zmax {zd:.2} off {od}");
    assert_eq!(op + od, 0, "no transition outside the machines");
    assert!(zp <= 3.5 && zd <= 3.5);
    // independence: joint of (next pitch, this duration) given (pitch, prev duration)
    let mut groups: std::collections::HashMap<(usize, usize), Vec<(usize, usize)>> = Default::default();
    for i in 1..idx.len() - 1 {
        groups.entry((ps[i], ds[i - 1])).or_default().push((ps[i + 1], ds[i]));
    }
    let mut zj = 0.0f64;
    for ((p0, u), l) in &groups {
        if l.len() < 400 {
            continue;
        }
        for t in 0..16 {
            for v in 0..5 {
                let q = pp.get(&(*p0, t)).unwrap_or(&0.0) * pd.get(&(*u, v)).unwrap_or(&0.0);
                let emp = l.iter().filter(|x| **x == (t, v)).count() as f64 / l.len() as f64;
                if q > 0.0 && q < 1.0 {
                    zj = zj.max((emp - q).abs() / (q * (1.0 - q) / l.len() as f64).sqrt());
                }
            }
        }
    }
    eprintln!("T1 joint zmax {zj:.2}");
    assert!(zj <= 3.5);
}

#[test]
fn t5_law_of_the_dual() {
    let (e, p) = play("t5_dual.score", 20_000, 7);
    let m = machines("t5_dual.machines");
    let (pp, pd) = (rownorm(&m["pitch"]), rownorm(&m["dur"]));
    let idx = indices(&e, &p);
    let ps: Vec<usize> = idx.iter().map(|x| x.0).collect();
    let ds: Vec<usize> = idx.iter().map(|x| x.1).collect();
    let (zp, op) = law_z(&ps, &pp, 16, 200);
    let (zd, od) = law_z(&ds, &pd, 5, 200);
    eprintln!("T5 pitch zmax {zp:.2}; dur zmax {zd:.2}");
    assert_eq!(op + od, 0);
    assert!(zp <= 3.5 && zd <= 3.5);
}

#[test]
fn t7_style_times_physicality() {
    let m = machines("t7.machines");
    let style = &m["pitch"];
    let mut stats = vec![];
    for (file, near) in [("t7_stepwise.score", true), ("t7_leaping.score", false)] {
        let (e, p) = play(file, 20_000, 21);
        let ps: Vec<usize> = indices(&e, &p).iter().map(|x| x.0).collect();
        let phys = |k: i64| -> f64 {
            if near {
                if k.abs() <= 2 { 3.0 } else { 0.3 }
            } else if [4, 7].contains(&k.abs()) {
                3.0
            } else {
                0.3
            }
        };
        let mut target = std::collections::HashMap::new();
        for s in 0..21 {
            let row: Vec<(usize, f64)> =
                style.iter().filter(|((a, _), _)| *a == s).map(|((_, t), w)| (*t, w * phys(*t as i64 - s as i64))).collect();
            let tot: f64 = row.iter().map(|x| x.1).sum();
            for (t, w) in row {
                target.insert((s, t), w / tot);
            }
        }
        let (z, off) = law_z(&ps, &target, 21, 200);
        let leaps: Vec<i64> = ps.windows(2).map(|w| (w[1] as i64 - w[0] as i64).abs()).collect();
        let mean = leaps.iter().sum::<i64>() as f64 / leaps.len() as f64;
        let step2 = leaps.iter().filter(|x| **x <= 2).count() as f64 / leaps.len() as f64;
        eprintln!("T7 {file}: zmax {z:.2} off {off} mean interval {mean:.2} share<=2 {step2:.3}");
        assert_eq!(off, 0);
        assert!(z <= 3.5);
        stats.push((mean, step2));
    }
    assert!(stats[0].1 > stats[1].1, "the stepwise instrument plays more steps");
}
