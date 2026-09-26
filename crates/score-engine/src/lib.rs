//! `score-engine`: runs a closed score. It knows nothing of voices, fans,
//! keys, servers, keyboards, players, synchronous output, chimeras, machines
//! or idioms (R-neutral): it matches receipts and messages on their subjects
//! (locations equivalent, timbres meeting concretely, data of opposite
//! polarity), forms contention sets, chooses a maximal matching with
//! probability proportional to the product of its clause values, fires it,
//! emits the records of the communications, and keeps musical time. Notes
//! are read off records by the playback function `score_core::nu`. No I/O.

pub mod explore;
pub mod matching;
pub mod soup;

use explore::Explorer;
use score_chance::{Chance, ChoiceCtx, Prng};
use score_core::digest::Digester;
use score_core::sha256::hex;
use score_core::*;
use score_logic::{Behaviour, Cfg, ClauseArena, Crisp, Graded, Locality, SlotView};
use soup::{full_weight, local_weight, CSet, Cand, Soup, Weigher};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

// ------------------------------------------------------------------ errors

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    /// a stable machine-readable kind, e.g. `freshness-violated`
    pub kind: &'static str,
    pub msg: String,
}
impl EngineError {
    pub fn new(kind: &'static str, msg: impl Into<String>) -> Self {
        EngineError { kind, msg: msg.into() }
    }
    pub fn eval(msg: impl Into<String>) -> Self {
        Self::new("evaluation", msg)
    }
    pub fn open() -> Self {
        Self::new("open-term", "a process with a free variable reached top level")
    }
    pub fn freshness() -> Self {
        Self::new(
            "freshness-violated",
            "a process was brought to top level at a collected location; this score is not fresh, \
             so `--gc freshness` (and behavioural exploration) is unsound for it",
        )
    }
    pub fn alternatives(n: usize) -> Self {
        Self::new(
            "too-many-alternatives",
            format!("a contention set has more than {n} alternatives (the player never truncates; raise --max-alternatives)"),
        )
    }
    pub fn is_replay_mismatch(&self) -> bool {
        self.kind == "replay-mismatch"
    }
}
impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.msg)
    }
}
impl std::error::Error for EngineError {}

// ------------------------------------------------------------------ config

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scheduler {
    /// the due set whose location has the least structural key
    Canonical,
    /// uniformly among due sets, from a dedicated PRNG
    Random(u64),
    /// by timbre priority, ties canonical
    ByTimbre(Vec<String>),
}
impl Scheduler {
    pub fn parse(s: &str) -> Result<Scheduler, String> {
        if s == "canonical" {
            return Ok(Scheduler::Canonical);
        }
        if let Some(x) = s.strip_prefix("random:") {
            return x.parse().map(Scheduler::Random).map_err(|_| format!("bad seed `{x}`"));
        }
        if let Some(x) = s.strip_prefix("by-timbre:") {
            return Ok(Scheduler::ByTimbre(x.split(',').map(|t| t.trim().to_string()).collect()));
        }
        Err(format!("unknown scheduler `{s}` (canonical, random:SEED, by-timbre:A,B,...)"))
    }
    pub fn spec(&self) -> String {
        match self {
            Scheduler::Canonical => "canonical".into(),
            Scheduler::Random(s) => format!("random:{s}"),
            Scheduler::ByTimbre(v) => format!("by-timbre:{}", v.join(",")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// a set fires a maximal matching (Definition 3.3)
    Max,
    /// a set fires one candidate (diagnostic only; T3)
    One,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub scheduler: Scheduler,
    pub mode: Mode,
    pub gc: bool,
    pub max_alternatives: usize,
    /// bound on consecutive zero-length notes at one onset
    pub unproductive: usize,
    pub max_states: usize,
    pub kmax: u32,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            scheduler: Scheduler::Canonical,
            mode: Mode::Max,
            gc: false,
            max_alternatives: 100_000,
            unproductive: 100_000,
            max_states: 200_000,
            kmax: score_chance::KMAX,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Limits {
    /// stop after this many notes of positive length
    pub notes: Option<u64>,
    /// stop before resolving a set whose onset is at or after this time
    pub until: Option<Q>,
    pub max_steps: Option<u64>,
}

// ----------------------------------------------------------------- sources

pub struct Sources {
    pub default: Box<dyn Chance>,
    pub streams: BTreeMap<String, Box<dyn Chance>>,
}
impl Sources {
    pub fn new(default: Box<dyn Chance>) -> Self {
        Sources { default, streams: BTreeMap::new() }
    }
    pub fn bind(&mut self, key: &str, c: Box<dyn Chance>) {
        self.streams.insert(key.to_string(), c);
    }
    fn get(&mut self, key: &str) -> &mut dyn Chance {
        match self.streams.get_mut(key) {
            Some(c) => c.as_mut(),
            None => self.default.as_mut(),
        }
    }
    pub fn describe(&self) -> Vec<(String, String)> {
        let mut v = vec![("*".to_string(), self.default.spec())];
        for (k, c) in &self.streams {
            v.push((k.clone(), c.spec()));
        }
        v
    }
}

// ------------------------------------------------------------------ output

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub onset: Q,
    pub timbre: Timbre,
    pub pitch: Datum,
    pub dur: Datum,
}

impl Note {
    /// The timed note of a record: its onset and `nu(record)`.
    pub fn of_record(onset: &Q, r: &Record, alph: &Alphabets) -> Note {
        let n = nu(r, alph).expect("every record the engine emits has a note (Prop. 2.10(i))");
        Note { onset: onset.clone(), timbre: n.timbre, pitch: n.pitch, dur: n.dur }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub step: u64,
    pub onset: Q,
    /// structural key of the location: a digest prefix of its quote
    pub loc: String,
    pub set_size: usize,
    pub alternatives: usize,
    pub chosen: usize,
    /// digits consumed per stream
    pub digits: Vec<(String, u32)>,
    /// "none" (one alternative), "joint", "factored", "human", "replay"
    pub routing: String,
    /// the records of the communications fired, with their onsets
    pub records: Vec<(Q, Record)>,
    /// structural keys (digest prefixes) of each record's location and
    /// payload process, for the trace
    pub record_keys: Vec<(String, String)>,
    /// their notes, computed by playback (`nu`) from `records` and nothing else
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// no contention set remains
    Quiescent,
    Notes,
    Until,
    Steps,
}

/// A recorded choice, for replay.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub step: u64,
    pub loc: String,
    pub alternatives: usize,
    pub chosen: usize,
    pub notes: Vec<Note>,
}

// ------------------------------------------------------------------ engine

pub struct Engine {
    pub arena: Arena,
    pub clauses: ClauseArena,
    pub alph: Alphabets,
    pub soup: Soup,
    pub config: Config,
    pub sources: Sources,
    sets: BTreeMap<Loc, Vec<CSet>>,
    dirty: BTreeSet<Loc>,
    beh_live: BTreeSet<Loc>,
    digester: Digester,
    explorer: Explorer,
    sched_rng: Option<Prng>,
    replay: Option<Vec<Recorded>>,
    replay_pos: usize,
    pub steps: u64,
    pub played: u64,
    zero_run: (Q, usize),
    pub behavioural: bool,
    pub window: u32,
    pub performance: Vec<Note>,
}

struct MapBehaviour<'a>(&'a HashMap<Cfg, bool>);
impl Behaviour for MapBehaviour<'_> {
    fn leads(&mut self, cfg: &Cfg) -> Result<bool, String> {
        self.0.get(cfg).copied().ok_or_else(|| "internal: leads formula not precomputed".into())
    }
}

fn collect_cfgs_g(g: &Graded, out: &mut Vec<Cfg>) {
    match g {
        Graded::Crisp(c) => collect_cfgs_c(c, out),
        Graded::Tensor(v) | Graded::Sum(v) => v.iter().for_each(|x| collect_cfgs_g(x, out)),
        _ => {}
    }
}
fn collect_cfgs_c(c: &Crisp, out: &mut Vec<Cfg>) {
    match c {
        Crisp::Not(a) => collect_cfgs_c(a, out),
        Crisp::And(v) | Crisp::Or(v) => v.iter().for_each(|x| collect_cfgs_c(x, out)),
        Crisp::Leads(cfg) => {
            if !out.contains(cfg) {
                out.push((**cfg).clone())
            }
        }
        _ => {}
    }
}

/// The weigher used by the engine: local clauses directly; behavioural ones
/// by deciding each `leads(...)` on the residue of the candidate.
struct EngineWeigher<'a> {
    explorer: &'a mut Explorer,
    pruned: Option<Soup>,
    bound: usize,
}
impl Weigher for EngineWeigher<'_> {
    fn weigh(
        &mut self,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        soup: &Soup,
        loc: Loc,
        clause: ClauseId,
        slots: &[SlotView],
        records: &[Record],
        recv: u64,
        sends: &[u64],
    ) -> Result<Q, EngineError> {
        let local = local_weight(arena, clauses, alph, loc, clause, slots)?;
        if local.is_zero() || clauses.info(clause).class == Locality::Local {
            return Ok(local);
        }
        if self.pruned.is_none() {
            let mut s = soup.clone();
            Explorer::prune(&mut s, arena, clauses, alph, None, self.bound)?;
            self.pruned = Some(s);
        }
        let mut cfgs = vec![];
        collect_cfgs_g(clauses.get(clause), &mut cfgs);
        let cand = Cand {
            recv,
            sends: sends.to_vec(),
            records: records.to_vec(),
            slots: slots.to_vec(),
            weight: Q::ONE,
            onset: Q::ZERO,
            stream: 0,
            clause,
        };
        let mut map = HashMap::new();
        for cfg in cfgs {
            let res = explore::residue(self.pruned.as_ref().unwrap(), loc, &cand, arena, clauses, alph, self.bound)?;
            let v = self.explorer.holds(res, &cfg, arena, clauses, alph)?;
            map.insert(cfg, v);
        }
        full_weight(arena, clauses, alph, loc, clause, slots, &mut MapBehaviour(&map))
    }
}

impl Engine {
    /// Build an engine from an elaborated score's parts.
    pub fn new(
        arena: Arena,
        clauses: ClauseArena,
        alph: Alphabets,
        initial: &[(Q, ProcId)],
        window: u32,
        behavioural: bool,
        config: Config,
        sources: Sources,
    ) -> Result<Engine, EngineError> {
        let sched_rng = match &config.scheduler {
            Scheduler::Random(s) => Some(Prng::new(*s)),
            _ => None,
        };
        let explorer = Explorer::new(window, config.max_states, config.max_alternatives);
        let mut e = Engine {
            arena,
            clauses,
            alph,
            soup: Soup::default(),
            config,
            sources,
            sets: BTreeMap::new(),
            dirty: BTreeSet::new(),
            beh_live: BTreeSet::new(),
            digester: Digester::new(),
            explorer,
            sched_rng,
            replay: None,
            replay_pos: 0,
            steps: 0,
            played: 0,
            zero_run: (Q::ZERO, 0),
            behavioural,
            window,
            performance: vec![],
        };
        let mut touched = BTreeSet::new();
        for (t, p) in initial {
            e.soup.spawn(&e.arena, &e.clauses, &e.alph, *p, t, None, &mut touched)?;
        }
        e.dirty = touched;
        Ok(e)
    }

    pub fn set_replay(&mut self, events: Vec<Recorded>) {
        self.replay = Some(events);
        self.replay_pos = 0;
    }

    pub fn regime(&self) -> String {
        if self.behavioural {
            format!("window-behavioural(depth {})", self.window)
        } else {
            "local".into()
        }
    }

    /// A structural key of a process: a prefix of its id-independent digest.
    pub fn proc_key(&mut self, p: ProcId) -> String {
        let d = self.digester.proc(&self.arena, &self.clauses, p);
        hex(&d)[..16].to_string()
    }

    pub fn loc_key(&mut self, l: Loc) -> String {
        let d = self.digester.proc(&self.arena, &self.clauses, l.quote);
        hex(&d)[..16].to_string()
    }

    fn refresh(&mut self) -> Result<(), EngineError> {
        let mut todo: BTreeSet<Loc> = std::mem::take(&mut self.dirty);
        todo.extend(self.beh_live.iter().copied());
        let mut w = EngineWeigher { explorer: &mut self.explorer, pruned: None, bound: self.config.max_alternatives };
        for l in todo {
            let cands = self.soup.candidates(
                &mut self.arena,
                &self.clauses,
                &self.alph,
                l,
                &mut w,
                self.config.max_alternatives,
                false,
            )?;
            let st = self.soup.locs.get(&l);
            let beh = st.map_or(false, |st| {
                !st.sends.is_empty()
                    && st.recvs.iter().any(|r| self.clauses.info(r.clause).class == Locality::Behavioural)
            });
            if beh {
                self.beh_live.insert(l);
            } else {
                self.beh_live.remove(&l);
            }
            if cands.is_empty() {
                self.sets.remove(&l);
            } else {
                self.sets.insert(l, Soup::contention_sets(cands));
            }
        }
        Ok(())
    }

    fn describe(&self, m: &[&Cand]) -> String {
        m.iter()
            .map(|c| {
                c.slots
                    .iter()
                    .map(|s| {
                        let carry = match s.carry {
                            Hat::Is(d) => self.alph.name(d).to_string(),
                            Hat::Wild => "_".into(),
                        };
                        format!("{} {} {} -> {}", self.alph.timbre_name(s.timbre), self.alph.name(s.pitch), self.alph.name(s.dur), carry)
                    })
                    .collect::<Vec<_>>()
                    .join(" & ")
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// Try the factorised resolution of Lemma 5.3. Returns the chosen index
    /// and digits consumed, or None when the set does not factor.
    fn factored(&mut self, set: &CSet, key: &str, onset: &Q, step: u64) -> Result<Option<(usize, Vec<(String, u32)>)>, EngineError> {
        let (kp, kd) = (format!("{key}.pitch"), format!("{key}.dur"));
        if !self.sources.streams.contains_key(&kp) || !self.sources.streams.contains_key(&kd) {
            return Ok(None);
        }
        let c0 = &set.cands[0];
        if set.cands.iter().any(|c| c.recv != c0.recv || c.slots.len() != 1) {
            return Ok(None);
        }
        let loc = match self.soup.locs.iter().find(|(_, st)| st.recvs.iter().any(|r| r.id == c0.recv)) {
            Some((l, _)) => *l,
            None => return Ok(None),
        };
        let recv_is_pitch = self.alph.is_pitch(c0.slots[0].pitch)
            && set.cands.iter().all(|c| c.slots[0].pitch == c0.slots[0].pitch);
        // pitch-side and duration-side values of each candidate
        if set.cands.iter().any(|c| c.slots[0].carry.is_wild()) {
            return Ok(None);
        }
        let side = |c: &Cand| -> (Datum, Datum) {
            let s = &c.slots[0];
            let carry = s.carry.concrete().expect("checked concrete");
            if recv_is_pitch {
                (carry, s.dur)
            } else {
                (s.pitch, carry)
            }
        };
        if set.cands.iter().any(|c| {
            let (a, b) = side(c);
            !self.alph.is_pitch(a) || self.alph.is_pitch(b)
        }) {
            return Ok(None);
        }
        let mut pf = vec![];
        let mut df = vec![];
        for (g, r) in self.clauses.factors(c0.clause) {
            if r.behaviour || r.passes || r.other_slots {
                return Ok(None);
            }
            let (reads_a, reads_b) = if recv_is_pitch { (r.carry, r.dur) } else { (r.pitch, r.carry) };
            match (reads_a, reads_b) {
                (true, true) => return Ok(None),
                (true, false) => pf.push(g),
                (false, true) => df.push(g),
                (false, false) => {}
            }
        }
        let a_vals: BTreeSet<Datum> = set.cands.iter().map(|c| side(c).0).collect();
        let b_vals: BTreeSet<Datum> = set.cands.iter().map(|c| side(c).1).collect();
        if a_vals.len() * b_vals.len() != set.cands.len() {
            return Ok(None);
        }
        let eval_prod = |fs: &[Graded], c: &Cand, arena: &Arena, alph: &Alphabets| -> Result<Q, EngineError> {
            let mut ev = score_logic::Evaluator::new(arena, alph);
            let view = score_logic::View { timbre: c.slots[0].timbre, loc: loc.quote, slots: &c.slots };
            let mut acc = Q::ONE;
            for f in fs {
                acc = acc.mul(&ev.graded(f, &view, &mut score_logic::LocalOnly));
            }
            if let Some(e) = ev.error {
                return Err(EngineError::eval(e));
            }
            Ok(acc)
        };
        let a_list: Vec<Datum> = a_vals.into_iter().collect();
        let b_list: Vec<Datum> = b_vals.into_iter().collect();
        let mut fw = vec![];
        for a in &a_list {
            let c = set.cands.iter().find(|c| side(c).0 == *a).unwrap();
            fw.push(eval_prod(&pf, c, &self.arena, &self.alph)?);
        }
        let mut gw = vec![];
        for b in &b_list {
            let c = set.cands.iter().find(|c| side(c).1 == *b).unwrap();
            gw.push(eval_prod(&df, c, &self.arena, &self.alph)?);
        }
        if fw.iter().chain(gw.iter()).any(|w| w.is_zero()) {
            return Ok(None);
        }
        let la: Vec<String> = a_list.iter().map(|d| self.alph.name(*d).to_string()).collect();
        let lb: Vec<String> = b_list.iter().map(|d| self.alph.name(*d).to_string()).collect();
        let ca = self.sources.get(&kp).choose(&fw, &ChoiceCtx { step, stream: &kp, onset, labels: &la }).map_err(EngineError::eval)?;
        let cb = self.sources.get(&kd).choose(&gw, &ChoiceCtx { step, stream: &kd, onset, labels: &lb }).map_err(EngineError::eval)?;
        let (a, b) = (a_list[ca.index], b_list[cb.index]);
        let idx = set.cands.iter().position(|c| side(c) == (a, b)).unwrap();
        Ok(Some((idx, vec![(kp, ca.digits), (kd, cb.digits)])))
    }

    /// Resolve one contention set. `Ok(None)` when nothing remains.
    pub fn step(&mut self, limits: &Limits) -> Result<Result<Event, Stop>, EngineError> {
        if let Some(m) = limits.max_steps {
            if self.steps >= m {
                return Ok(Err(Stop::Steps));
            }
        }
        if let Some(n) = limits.notes {
            if self.played >= n {
                return Ok(Err(Stop::Notes));
            }
        }
        self.refresh()?;
        let tmin = match self.sets.values().flatten().map(|s| &s.onset).min() {
            Some(t) => t.clone(),
            None => return Ok(Err(Stop::Quiescent)),
        };
        if let Some(u) = &limits.until {
            if &tmin >= u {
                return Ok(Err(Stop::Until));
            }
        }
        let due: Vec<(Loc, usize)> = self
            .sets
            .iter()
            .flat_map(|(l, v)| v.iter().enumerate().filter(|(_, s)| s.onset == tmin).map(move |(i, _)| (*l, i)))
            .collect();
        // scheduling
        // the timbre of a set, for by-timbre scheduling, is its least
        // candidate's note's timbre
        let mut keyed: Vec<([u8; 32], u16, usize, (Loc, usize))> = due
            .into_iter()
            .map(|(l, i)| {
                let d = self.digester.proc(&self.arena, &self.clauses, l.quote);
                let t = self.sets[&l][i].cands[0].slots[0].timbre.0;
                (d, t, i, (l, i))
            })
            .collect();
        keyed.sort();
        let (loc, si) = match &self.config.scheduler {
            Scheduler::Canonical => keyed[0].3,
            Scheduler::Random(_) => {
                let k = self.sched_rng.as_mut().unwrap().below(keyed.len() as u64) as usize;
                keyed[k].3
            }
            Scheduler::ByTimbre(order) => {
                let alph = &self.alph;
                let prio = |t: u16| {
                    order.iter().position(|n| alph.lookup_timbre(n) == Some(Timbre(t))).unwrap_or(usize::MAX)
                };
                keyed.iter().min_by_key(|k| (prio(k.1), k.0, k.2)).unwrap().3
            }
        };
        let set = self.sets.get(&loc).unwrap()[si].clone();
        let step = self.steps;
        // alternatives
        let alts: Vec<Vec<usize>> = match self.config.mode {
            Mode::Max => matching::maximal_matchings(&set.cands, self.config.max_alternatives)?,
            Mode::One => (0..set.cands.len()).map(|i| vec![i]).collect(),
        };
        let weights: Vec<Q> = alts
            .iter()
            .map(|m| m.iter().fold(Q::ONE, |acc, &i| acc.mul(&set.cands[i].weight)))
            .collect();
        let stream = self.soup.streams[set.cands[0].stream as usize].clone();
        let loc_key = self.loc_key(loc);
        let mut digits = vec![];
        let routing;
        let chosen;
        if alts.len() == 1 {
            chosen = 0;
            routing = "none".to_string();
        } else if let Some(rep) = &self.replay {
            let r = rep.get(self.replay_pos).ok_or_else(|| {
                EngineError::new("replay-mismatch", format!("step {step}: the trace has no more choices"))
            })?;
            if r.step != step || r.loc != loc_key || r.alternatives != alts.len() {
                return Err(EngineError::new(
                    "replay-mismatch",
                    format!(
                        "step {step}: the trace chose at step {} in {} among {}, the run is at {} among {}",
                        r.step,
                        r.loc,
                        r.alternatives,
                        loc_key,
                        alts.len()
                    ),
                ));
            }
            chosen = r.chosen;
            routing = "replay".into();
        } else if self.config.mode == Mode::Max && alts.iter().all(|m| m.len() == 1) {
            match self.factored(&set, &stream, &tmin, step)? {
                Some((idx, d)) => {
                    chosen = alts.iter().position(|m| m[0] == idx).unwrap();
                    digits = d;
                    routing = "factored".into();
                }
                None => {
                    let labels: Vec<String> = alts.iter().map(|m| self.describe(&m.iter().map(|&i| &set.cands[i]).collect::<Vec<_>>())).collect();
                    let c = self.sources.get(&stream).choose(&weights, &ChoiceCtx { step, stream: &stream, onset: &tmin, labels: &labels }).map_err(EngineError::eval)?;
                    chosen = c.index;
                    digits.push((stream.clone(), c.digits));
                    routing = "joint".into();
                }
            }
        } else {
            let labels: Vec<String> = alts.iter().map(|m| self.describe(&m.iter().map(|&i| &set.cands[i]).collect::<Vec<_>>())).collect();
            let c = self.sources.get(&stream).choose(&weights, &ChoiceCtx { step, stream: &stream, onset: &tmin, labels: &labels }).map_err(EngineError::eval)?;
            chosen = c.index;
            digits.push((stream.clone(), c.digits));
            routing = "joint".into();
        }
        if chosen >= alts.len() {
            return Err(EngineError::new("bad-choice", format!("choice {chosen} out of {} alternatives", alts.len())));
        }
        let matching: Vec<Cand> = alts[chosen].iter().map(|&i| set.cands[i].clone()).collect();
        let mut touched = BTreeSet::new();
        let records = self.soup.fire(&mut self.arena, &self.clauses, &self.alph, loc, &matching, &mut touched)?;
        // playback: the notes are read off the records and nothing else
        let notes: Vec<Note> = records.iter().map(|(t, r)| Note::of_record(t, r, &self.alph)).collect();
        let record_keys: Vec<(String, String)> =
            records.iter().map(|(_, r)| (self.proc_key(r.loc), self.proc_key(r.payload))).collect();
        if self.replay.is_some() && routing == "replay" {
            let r = &self.replay.as_ref().unwrap()[self.replay_pos];
            if r.notes != notes {
                return Err(EngineError::new("replay-mismatch", format!("step {step}: the replayed choice emitted different notes")));
            }
            self.replay_pos += 1;
        }
        // productivity
        for n in &notes {
            if self.alph.len(n.dur).is_positive() {
                self.played += 1;
                self.performance.push(n.clone());
                self.zero_run = (n.onset.clone(), 0);
            } else {
                if self.zero_run.0 != n.onset {
                    self.zero_run = (n.onset.clone(), 0);
                }
                self.zero_run.1 += 1;
                if self.zero_run.1 > self.config.unproductive {
                    return Err(EngineError::new(
                        "unproductive",
                        format!(
                            "more than {} consecutive zero-length notes at onset {} (last at location {loc_key}); \
                             the note gives no syntactic productivity check yet (open thread 6)",
                            self.config.unproductive, n.onset
                        ),
                    ));
                }
            }
        }
        self.dirty.extend(touched);
        // freshness collection of the fired record location
        if self.config.gc && Soup::is_record(&self.arena, loc) {
            self.refresh()?;
            if !self.sets.contains_key(&loc) {
                self.soup.collect(loc);
                self.beh_live.remove(&loc);
                self.dirty.remove(&loc);
            }
        }
        self.steps += 1;
        Ok(Ok(Event {
            step,
            onset: tmin,
            loc: loc_key,
            set_size: set.cands.len(),
            alternatives: alts.len(),
            chosen,
            digits,
            routing,
            records,
            record_keys,
            notes,
        }))
    }

    /// Run to a stop, handing each event to `sink`.
    pub fn run(&mut self, limits: &Limits, sink: &mut dyn FnMut(&Event, &Engine)) -> Result<Stop, EngineError> {
        loop {
            match self.step(limits)? {
                Ok(ev) => sink(&ev, self),
                Err(stop) => return Ok(stop),
            }
        }
    }

    /// The performance sorted by (onset, timbre, pitch, duration).
    pub fn sorted_performance(&self) -> Vec<Note> {
        let mut v = self.performance.clone();
        v.sort_by(|a, b| (&a.onset, a.timbre, a.pitch, a.dur).cmp(&(&b.onset, b.timbre, b.pitch, b.dur)));
        v
    }

    /// Locations currently holding at least one candidate of positive weight.
    pub fn live_locations(&mut self) -> Result<Vec<Loc>, EngineError> {
        self.refresh()?;
        Ok(self.sets.keys().copied().collect())
    }

    pub fn explorer_states(&self) -> usize {
        self.explorer.states()
    }
}
