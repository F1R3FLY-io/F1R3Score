//! The soup: top-level receipts and messages indexed by location, candidate
//! generation, contention sets and firing. Nothing here knows about voices.

use crate::EngineError;
use score_core::*;
use score_core::digest::Digester;
use score_core::sha256::hex;
use score_logic::{Behaviour, ClauseArena, Evaluator, LocalOnly, SlotView, View};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A top-level receipt occurrence. A join has one subject decoration per
/// pattern, all at the same location. Each occurrence keeps its own subject
/// timbre, which may be `_` (R-locindex).
#[derive(Clone, Debug)]
pub struct ROcc {
    pub id: u64,
    pub subjects: Vec<Deco>,
    pub clause: ClauseId,
    pub body: ProcId,
    pub stamp: Q,
    /// index into `Soup::streams`
    pub stream: u32,
}

/// A top-level message occurrence.
#[derive(Clone, Debug)]
pub struct SOcc {
    pub id: u64,
    pub subj: Deco,
    pub payload: ProcId,
    pub ptimbre: Hat<Timbre>,
    pub carry: Hat<Datum>,
    pub stamp: Q,
}

#[derive(Clone, Debug, Default)]
pub struct LocState {
    pub recvs: Vec<ROcc>,
    pub sends: Vec<SOcc>,
}

/// A candidate: a receipt occurrence and one message per pattern, with the
/// prospective record of each pattern and the view built from it.
#[derive(Clone, Debug)]
pub struct Cand {
    pub recv: u64,
    pub sends: Vec<u64>,
    pub records: Vec<Record>,
    pub slots: Vec<SlotView>,
    pub weight: Q,
    pub onset: Q,
    pub stream: u32,
    pub clause: ClauseId,
}

#[derive(Clone, Debug)]
pub struct CSet {
    /// in canonical order: by receipt id, then message ids
    pub cands: Vec<Cand>,
    pub onset: Q,
}

#[derive(Clone, Default)]
pub struct Soup {
    pub locs: BTreeMap<Loc, LocState>,
    pub next_id: u64,
    pub tombstones: BTreeSet<Loc>,
    pub streams: Vec<String>,
    pub stream_index: HashMap<String, u32>,
    /// default streams of open-timbre receipts, by location (cached digest)
    pub loc_streams: BTreeMap<ProcId, u32>,
}

/// How the weight of a candidate is obtained.
pub trait Weigher {
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
    ) -> Result<Q, EngineError>;
}

/// The local weight: the clause with every behavioural formula dropped (its
/// top-level behavioural factors are ignored). Used for availability inside
/// behavioural exploration, and for every clause of a local score.
pub struct LocalWeigher;
impl Weigher for LocalWeigher {
    fn weigh(
        &mut self,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        _: &Soup,
        loc: Loc,
        clause: ClauseId,
        slots: &[SlotView],
        _: &[Record],
        _: u64,
        _: &[u64],
    ) -> Result<Q, EngineError> {
        local_weight(arena, clauses, alph, loc, clause, slots)
    }
}

pub fn local_weight(
    arena: &Arena,
    clauses: &ClauseArena,
    alph: &Alphabets,
    loc: Loc,
    clause: ClauseId,
    slots: &[SlotView],
) -> Result<Q, EngineError> {
    let mut ev = Evaluator::new(arena, alph);
    let view = View { timbre: slots[0].timbre, loc: loc.quote, slots };
    let w = if clauses.info(clause).class == score_logic::Locality::Local {
        ev.graded(clauses.plan(clause), &view, &mut LocalOnly)
    } else {
        let mut acc = Q::ONE;
        for (g, r) in clauses.factors(clause) {
            if r.behaviour {
                continue;
            }
            let x = ev.graded(&g, &view, &mut LocalOnly);
            if x.is_zero() {
                acc = Q::ZERO;
                break;
            }
            acc = acc.mul(&x);
        }
        acc
    };
    if let Some(e) = ev.error {
        return Err(EngineError::eval(e));
    }
    Ok(w)
}

/// Evaluate a whole clause with a behavioural oracle.
pub fn full_weight(
    arena: &Arena,
    clauses: &ClauseArena,
    alph: &Alphabets,
    loc: Loc,
    clause: ClauseId,
    slots: &[SlotView],
    b: &mut dyn Behaviour,
) -> Result<Q, EngineError> {
    let mut ev = Evaluator::new(arena, alph);
    let view = View { timbre: slots[0].timbre, loc: loc.quote, slots };
    let w = ev.graded(clauses.plan(clause), &view, b);
    if let Some(e) = ev.error {
        return Err(EngineError::eval(e));
    }
    Ok(w)
}

impl Soup {
    pub fn stream_id(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.stream_index.get(name) {
            return i;
        }
        let i = self.streams.len() as u32;
        self.streams.push(name.to_string());
        self.stream_index.insert(name.to_string(), i);
        i
    }

    fn fresh(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The default stream of a receipt with no label and nothing inherited:
    /// its subject timbre when that is concrete, else the digest of its
    /// location (Decision "stream labels").
    fn default_stream(&mut self, arena: &Arena, clauses: &ClauseArena, alph: &Alphabets, loc: Loc, t: Hat<Timbre>) -> u32 {
        if let Hat::Is(t) = t {
            return self.stream_id(&alph.timbre_name(t).to_string());
        }
        if let Some(&s) = self.loc_streams.get(&loc.quote) {
            return s;
        }
        let d = Digester::new().proc(arena, clauses, loc.quote);
        let s = self.stream_id(&format!("loc:{}", &hex(&d)[..16]));
        self.loc_streams.insert(loc.quote, s);
        s
    }

    /// Bring a closed process to top level with a stamp. Records the touched
    /// locations.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        arena: &Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        p: ProcId,
        stamp: &Q,
        inherited: Option<u32>,
        touched: &mut BTreeSet<Loc>,
    ) -> Result<(), EngineError> {
        let mut stack = vec![p];
        while let Some(p) = stack.pop() {
            match arena.get(p) {
                Proc::Nil => {}
                Proc::Par(cs) => stack.extend(cs.iter().rev().copied()),
                Proc::Recv { subjects, clause, body, label } => {
                    let mut loc = None;
                    let mut decos = vec![];
                    for n in subjects.iter() {
                        let Name::Quote { proc, timbre, datum } = *n else {
                            return Err(EngineError::open());
                        };
                        let l = Loc { quote: proc };
                        if loc.is_some() && loc != Some(l) {
                            return Err(EngineError::new(
                                "join-across-locations",
                                "a chord receipt's patterns must all be at one location (Definition 6.5)",
                            ));
                        }
                        loc = Some(l);
                        decos.push(Deco { timbre, datum });
                    }
                    let loc = loc.unwrap();
                    if decos.iter().any(|d| d.timbre == Hat::Is(Timbre::DEAD)) {
                        // bases: receipts on the dead channel never run
                        continue;
                    }
                    if self.tombstones.contains(&loc) {
                        return Err(EngineError::freshness());
                    }
                    let stream = match label {
                        Some(l) => self.stream_id(&arena.label_name(*l).to_string()),
                        None => match inherited {
                            Some(s) => s,
                            None => self.default_stream(arena, clauses, alph, loc, decos[0].timbre),
                        },
                    };
                    let id = self.fresh();
                    let (clause, body) = (*clause, *body);
                    self.locs.entry(loc).or_default().recvs.push(ROcc {
                        id,
                        subjects: decos,
                        clause,
                        body,
                        stamp: stamp.clone(),
                        stream,
                    });
                    touched.insert(loc);
                }
                Proc::Send { subj, payload, ptimbre, carry } => {
                    let Name::Quote { proc, timbre, datum } = *subj else {
                        return Err(EngineError::open());
                    };
                    let loc = Loc { quote: proc };
                    if self.tombstones.contains(&loc) {
                        return Err(EngineError::freshness());
                    }
                    let id = self.fresh();
                    let (payload, ptimbre, carry) = (*payload, *ptimbre, *carry);
                    self.locs.entry(loc).or_default().sends.push(SOcc {
                        id,
                        subj: Deco { timbre, datum },
                        payload,
                        ptimbre,
                        carry,
                        stamp: stamp.clone(),
                    });
                    touched.insert(loc);
                }
                Proc::Drop(_) => return Err(EngineError::open()),
            }
        }
        Ok(())
    }

    /// All candidates at a location with their weights (zero included only
    /// when `keep_zero`). Canonical order.
    #[allow(clippy::too_many_arguments)]
    pub fn candidates(
        &self,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        loc: Loc,
        weigher: &mut dyn Weigher,
        max_alternatives: usize,
        keep_zero: bool,
    ) -> Result<Vec<Cand>, EngineError> {
        let Some(st) = self.locs.get(&loc) else { return Ok(vec![]) };
        let mut out = vec![];
        for r in &st.recvs {
            let k = r.subjects.len();
            // messages whose subject matches each pattern (Defs. 2.4, 2.7):
            // timbres meet concretely, data concrete and of opposite polarity.
            // The payload takes no part.
            let mut usable: Vec<Vec<usize>> = vec![vec![]; k];
            for (pi, d) in r.subjects.iter().enumerate() {
                for (si, s) in st.sends.iter().enumerate() {
                    if subjects_match(*d, s.subj, alph).is_some() {
                        usable[pi].push(si);
                    }
                }
            }
            // injective assignments
            let mut assign: Vec<usize> = vec![];
            let mut all: Vec<Vec<usize>> = vec![];
            fn rec(
                i: usize,
                usable: &[Vec<usize>],
                assign: &mut Vec<usize>,
                all: &mut Vec<Vec<usize>>,
                bound: usize,
            ) -> bool {
                if i == usable.len() {
                    all.push(assign.clone());
                    return all.len() <= bound;
                }
                for &s in &usable[i] {
                    if assign.contains(&s) {
                        continue;
                    }
                    assign.push(s);
                    let ok = rec(i + 1, usable, assign, all, bound);
                    assign.pop();
                    if !ok {
                        return false;
                    }
                }
                true
            }
            if !rec(0, &usable, &mut assign, &mut all, max_alternatives) {
                return Err(EngineError::alternatives(max_alternatives));
            }
            for a in all {
                let mut records = vec![];
                let mut slots = vec![];
                let mut onset = r.stamp.clone();
                for (pi, &si) in a.iter().enumerate() {
                    let s = &st.sends[si];
                    let rec = Record {
                        loc: loc.quote,
                        recv: r.subjects[pi],
                        send: s.subj,
                        payload: s.payload,
                        ptimbre: s.ptimbre,
                        carry: s.carry,
                    };
                    slots.push(SlotView::of_record(&rec, alph).expect("a matching pair has a note (Prop. 2.10(i))"));
                    records.push(rec);
                    onset = Q::max(&onset, &s.stamp);
                }
                let sends: Vec<u64> = a.iter().map(|&si| st.sends[si].id).collect();
                let w = weigher.weigh(arena, clauses, alph, self, loc, r.clause, &slots, &records, r.id, &sends)?;
                if w.is_positive() || keep_zero {
                    out.push(Cand { recv: r.id, sends, records, slots, weight: w, onset, stream: r.stream, clause: r.clause });
                }
            }
        }
        out.sort_by(|a, b| (a.recv, &a.sends).cmp(&(b.recv, &b.sends)));
        Ok(out)
    }

    /// Contention sets: components of "shares a receipt or a message".
    pub fn contention_sets(cands: Vec<Cand>) -> Vec<CSet> {
        let n = cands.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut by: HashMap<(bool, u64), usize> = HashMap::new();
        for (i, c) in cands.iter().enumerate() {
            let keys = std::iter::once((true, c.recv)).chain(c.sends.iter().map(|s| (false, *s)));
            for k in keys {
                match by.get(&k) {
                    Some(&j) => {
                        let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                        parent[a] = b;
                    }
                    None => {
                        by.insert(k, i);
                    }
                }
            }
        }
        let mut groups: BTreeMap<usize, Vec<Cand>> = BTreeMap::new();
        let mut first: HashMap<usize, usize> = HashMap::new();
        for (i, c) in cands.into_iter().enumerate() {
            let r = find(&mut parent, i);
            let key = *first.entry(r).or_insert(i);
            groups.entry(key).or_default().push(c);
        }
        groups
            .into_values()
            .map(|cands| {
                let onset = cands.iter().map(|c| c.onset.clone()).min().unwrap();
                CSet { cands, onset }
            })
            .collect()
    }

    /// Fire a matching at a location: remove its occurrences, and bring each
    /// continuation `P{<@Q, t3, d>/y}` to top level. Returns the records of
    /// the communications with their onsets; nothing else is kept (reduction
    /// is unlabelled, R-record).
    pub fn fire(
        &mut self,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        loc: Loc,
        matching: &[Cand],
        touched: &mut BTreeSet<Loc>,
    ) -> Result<Vec<(Q, Record)>, EngineError> {
        let st = self.locs.get_mut(&loc).expect("location of a candidate");
        let mut bodies = vec![];
        for c in matching {
            let ri = st.recvs.iter().position(|r| r.id == c.recv).expect("receipt present");
            let r = st.recvs.remove(ri);
            for sid in &c.sends {
                let si = st.sends.iter().position(|s| s.id == *sid).expect("message present");
                st.sends.remove(si);
            }
            bodies.push((r, c.clone()));
        }
        touched.insert(loc);
        let mut out = vec![];
        for (r, c) in bodies {
            let mut names = vec![];
            let mut longest = Q::ZERO;
            for (rec, s) in c.records.iter().zip(c.slots.iter()) {
                out.push((c.onset.clone(), *rec));
                // the payload is carried across unchanged
                names.push(Name::Quote { proc: rec.payload, timbre: rec.ptimbre, datum: rec.carry });
                longest = Q::max(&longest, &alph.len(s.dur));
            }
            let cont = arena.instantiate(r.body, &names);
            let t = chord_release(&c.onset, &longest);
            self.spawn(arena, clauses, alph, cont, &t, Some(r.stream), touched)?;
        }
        Ok(out)
    }

    /// Drop a location and remember it (freshness collection).
    pub fn collect(&mut self, loc: Loc) {
        self.locs.remove(&loc);
        self.tombstones.insert(loc);
    }

    /// Is the location record-shaped (R-gc)? Only such locations may be
    /// collected; key and acknowledgement locations never are.
    pub fn is_record(arena: &Arena, loc: Loc) -> bool {
        arena.is_record_shaped(loc.quote)
    }
}

/// When a firing's continuation starts: onset plus the longest of its
/// durations. For a plain receipt that is its one duration; for a chord it is
/// the "longest duration" convention of Definition 6.5, kept in this one
/// function because the note lists it as open thread 5.
pub fn chord_release(onset: &Q, longest: &Q) -> Q {
    onset.add(longest)
}
