//! Helpers for the conformance suite: load a `.score` file and run it.

pub use score_chance as chance;
pub use score_core as core;
pub use score_engine as engine;
pub use score_syntax as syntax;

use score_engine::*;
use std::path::{Path, PathBuf};

pub fn scores_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scores")
}

pub struct FsLoader(pub PathBuf);
impl score_syntax::Loader for FsLoader {
    fn load(&mut self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| e.to_string())
    }
}

pub fn load_src(name: &str, src: &str) -> score_syntax::Score {
    score_syntax::load(name, src, &mut FsLoader(scores_dir())).unwrap_or_else(|e| panic!("{e}"))
}

pub fn load_file(name: &str) -> score_syntax::Score {
    let src = std::fs::read_to_string(scores_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    load_src(name, &src)
}

pub fn engine(s: score_syntax::Score, config: Config, sources: Sources) -> Engine {
    Engine::new(s.arena, s.clauses, s.alph, &s.initial, s.window, s.behavioural, config, sources)
        .unwrap_or_else(|e| panic!("{e}"))
}

pub fn prng(seed: u64) -> Box<dyn score_chance::Chance> {
    score_chance::source(&format!("prng:{seed}"), score_chance::KMAX).unwrap()
}

/// Run and collect the events.
pub fn run(e: &mut Engine, limits: &Limits) -> (Stop, Vec<Event>) {
    let mut evs = vec![];
    let stop = e.run(limits, &mut |ev, _| evs.push(ev.clone())).unwrap_or_else(|err| panic!("{err}"));
    (stop, evs)
}

/// Pitch and duration names of a note.
pub fn names(e: &Engine, n: &Note) -> (String, String) {
    (e.alph.name(n.pitch).to_string(), e.alph.name(n.dur).to_string())
}

// ------------------------------------------------------------- statistics

use std::collections::HashMap;

/// Machines written by gen_scores.py: kind -> (s, t) -> weight.
pub fn machines(name: &str) -> HashMap<String, HashMap<(usize, usize), f64>> {
    let src = std::fs::read_to_string(scores_dir().join(name)).unwrap();
    let mut m: HashMap<String, HashMap<(usize, usize), f64>> = HashMap::new();
    for l in src.lines() {
        let v: Vec<&str> = l.split_whitespace().collect();
        if v.len() == 4 {
            m.entry(v[0].into()).or_default().insert((v[1].parse().unwrap(), v[2].parse().unwrap()), v[3].parse().unwrap());
        }
    }
    m
}

pub fn rownorm(e: &HashMap<(usize, usize), f64>) -> HashMap<(usize, usize), f64> {
    let mut tot: HashMap<usize, f64> = HashMap::new();
    for ((s, _), w) in e {
        *tot.entry(*s).or_default() += w;
    }
    e.iter().map(|((s, t), w)| ((*s, *t), w / tot[s])).collect()
}

/// Largest |z| over machine cells with 0 < p < 1 in rows seen at least
/// `min_row` times, and the number of observed off-machine transitions.
pub fn law_z(seq: &[usize], p: &HashMap<(usize, usize), f64>, n: usize, min_row: usize) -> (f64, usize) {
    let mut c: HashMap<(usize, usize), usize> = HashMap::new();
    for w in seq.windows(2) {
        *c.entry((w[0], w[1])).or_default() += 1;
    }
    let (mut zmax, mut off) = (0.0f64, 0);
    for s in 0..n {
        let tot: usize = (0..n).map(|t| c.get(&(s, t)).copied().unwrap_or(0)).sum();
        if tot < min_row {
            continue;
        }
        for t in 0..n {
            let e = c.get(&(s, t)).copied().unwrap_or(0) as f64 / tot as f64;
            let q = p.get(&(s, t)).copied().unwrap_or(0.0);
            if q == 0.0 && e > 0.0 {
                off += 1;
            }
            if q > 0.0 && q < 1.0 {
                zmax = zmax.max((e - q).abs() / (q * (1.0 - q) / tot as f64).sqrt());
            }
        }
    }
    (zmax, off)
}

/// (pitch index, duration index) of each performed note.
pub fn indices(e: &Engine, notes: &[Note]) -> Vec<(usize, usize)> {
    let np = e.alph.n_pitches();
    notes.iter().map(|n| (n.pitch.0 as usize, n.dur.0 as usize - np)).collect()
}
