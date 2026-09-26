//! Evaluation of clauses on candidate views (R-exact, R-pure).

use crate::formula::*;
use score_core::{Alphabets, Arena, Datum, Name, Proc, ProcId, Timbre, Q};

/// One pattern of a candidate: the note it would emit, the datum carried
/// forward and the process passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotView {
    pub pitch: Datum,
    pub dur: Datum,
    pub carry: Datum,
    pub passes: ProcId,
}

/// What a clause sees of a candidate (Definition 3.1 of the note).
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub timbre: Timbre,
    /// the quote of the location at which the candidate meets
    pub loc: ProcId,
    pub slots: &'a [SlotView],
}

/// The behavioural oracle: evaluates a configuration formula on the residue
/// of firing the candidate under evaluation. Supplied by the engine.
pub trait Behaviour {
    fn leads(&mut self, cfg: &Cfg) -> Result<bool, String>;
}

/// Treats every `leads(...)` as true: the local part of a clause.
pub struct LocalOnly;
impl Behaviour for LocalOnly {
    fn leads(&mut self, _: &Cfg) -> Result<bool, String> {
        Ok(true)
    }
}

/// Refuses behavioural formulae (for contexts where none may appear).
pub struct NoBehaviour;
impl Behaviour for NoBehaviour {
    fn leads(&mut self, _: &Cfg) -> Result<bool, String> {
        Err("a behavioural formula was evaluated where none is admitted".into())
    }
}

pub struct Evaluator<'a> {
    pub arena: &'a Arena,
    pub alph: &'a Alphabets,
    /// bound on the number of splits a separating conjunction may enumerate
    pub max_splits: u64,
    /// set when evaluation could not be completed exactly
    pub error: Option<String>,
}

impl<'a> Evaluator<'a> {
    pub fn new(arena: &'a Arena, alph: &'a Alphabets) -> Self {
        Evaluator { arena, alph, max_splits: 1 << 16, error: None }
    }

    fn slot<'v>(&self, v: &View<'v>, s: u8) -> Option<&'v SlotView> {
        v.slots.get(s as usize)
    }

    pub fn key(&self, f: KeyFn, v: &View) -> KeyVal {
        match f {
            KeyFn::Pitch(s) => self.slot(v, s).map(|x| KeyVal::D(x.pitch)).unwrap_or(KeyVal::Undef),
            KeyFn::Dur(s) => self.slot(v, s).map(|x| KeyVal::D(x.dur)).unwrap_or(KeyVal::Undef),
            KeyFn::Carry(s) => self.slot(v, s).map(|x| KeyVal::D(x.carry)).unwrap_or(KeyVal::Undef),
            KeyFn::Prev => match self.arena.get(v.loc) {
                Proc::Send { subj: Name::Quote { datum, .. }, .. } => KeyVal::D(*datum),
                _ => KeyVal::Undef,
            },
            KeyFn::Held => match self.arena.get(v.loc) {
                Proc::Send { carry, .. } => KeyVal::D(*carry),
                _ => KeyVal::Undef,
            },
            KeyFn::Step(s) => match self.slot(v, s) {
                Some(x) => match (self.alph.ord(x.carry), self.alph.ord(x.pitch)) {
                    (Some(c), Some(p)) => KeyVal::I(c - p),
                    _ => KeyVal::Undef,
                },
                None => KeyVal::Undef,
            },
        }
    }

    pub fn table(&self, t: &Table, v: &View) -> Q {
        let mut k = [KeyVal::Pad; 3];
        for (i, f) in t.key.iter().enumerate() {
            k[i] = self.key(*f, v);
        }
        t.entries.get(&k).cloned().unwrap_or_else(|| t.default.clone())
    }

    pub fn graded(&mut self, g: &Graded, v: &View, b: &mut dyn Behaviour) -> Q {
        match g {
            Graded::Const(w) => w.clone(),
            Graded::Crisp(c) => {
                if self.crisp(c, v, b) {
                    Q::ONE
                } else {
                    Q::ZERO
                }
            }
            Graded::Table(t) => self.table(t, v),
            Graded::Tensor(fs) => {
                let mut acc = Q::ONE;
                for f in fs {
                    let x = self.graded(f, v, b);
                    if x.is_zero() {
                        return Q::ZERO;
                    }
                    acc = acc.mul(&x);
                }
                acc
            }
            Graded::Sum(fs) => {
                let mut acc = Q::ZERO;
                for f in fs {
                    acc = acc.add(&self.graded(f, v, b));
                }
                acc
            }
        }
    }

    pub fn crisp(&mut self, c: &Crisp, v: &View, b: &mut dyn Behaviour) -> bool {
        match c {
            Crisp::True => true,
            Crisp::False => false,
            Crisp::Not(a) => !self.crisp(a, v, b),
            Crisp::And(xs) => xs.iter().all(|x| self.crisp(x, v, b)),
            Crisp::Or(xs) => xs.iter().any(|x| self.crisp(x, v, b)),
            Crisp::Atom(a) => self.atom(a, v),
            Crisp::Leads(cfg) => match b.leads(cfg) {
                Ok(x) => x,
                Err(e) => {
                    self.error.get_or_insert(e);
                    false
                }
            },
        }
    }

    pub fn atom(&mut self, a: &Atom, v: &View) -> bool {
        match a {
            Atom::Pitch(s, d) => self.slot(v, *s).map(|x| x.pitch == *d).unwrap_or(false),
            Atom::Dur(s, d) => self.slot(v, *s).map(|x| x.dur == *d).unwrap_or(false),
            Atom::Carry(s, d) => self.slot(v, *s).map(|x| x.carry == *d).unwrap_or(false),
            Atom::Step(s, k) => self.key(KeyFn::Step(*s), v) == KeyVal::I(*k),
            Atom::At(sp) => self.sp(sp, v.loc),
            Atom::Passes(s, sp) => match self.slot(v, *s) {
                Some(x) => self.sp(sp, x.passes),
                None => false,
            },
        }
    }

    // ------------------------------------------------------------- spatial

    /// Does the process `p` satisfy the spatial formula?
    pub fn sp(&mut self, s: &Sp, p: ProcId) -> bool {
        match s {
            Sp::True => true,
            Sp::Zero => matches!(self.arena.get(p), Proc::Nil),
            Sp::Not(a) => !self.sp(a, p),
            Sp::And(xs) => xs.iter().all(|x| self.sp(x, p)),
            Sp::Or(xs) => xs.iter().any(|x| self.sp(x, p)),
            Sp::Par(..) => {
                let cs = self.arena.components(p);
                self.sp_multi(s, &cs)
            }
            Sp::Out { subj, payload, ptimbre, carry } => match self.arena.get(p) {
                Proc::Send { subj: n, payload: pl, ptimbre: pt, carry: c } => {
                    let (n, pl, pt, c) = (*n, *pl, *pt, *c);
                    ptimbre.map_or(true, |t| t == pt)
                        && carry.map_or(true, |d| d == c)
                        && self.name(subj, n)
                        && self.sp(payload, pl)
                }
                _ => false,
            },
            Sp::In { subj, body } => match self.arena.get(p) {
                Proc::Recv { subjects, body: bd, .. } if subjects.len() == 1 => {
                    let (n, bd) = (subjects[0], *bd);
                    self.name(subj, n) && self.sp(body, bd)
                }
                _ => false,
            },
        }
    }

    fn name(&mut self, pat: &NamePat, n: Name) -> bool {
        match n {
            Name::Quote { proc, timbre, datum } => {
                pat.timbre.map_or(true, |t| t == timbre)
                    && pat.datum.map_or(true, |d| d == datum)
                    && pat.quote.as_ref().map_or(true, |q| self.sp(q, proc))
            }
            // an open name (inside quoted code) matches only the wildcard
            _ => pat.quote.is_none() && pat.timbre.is_none() && pat.datum.is_none(),
        }
    }

    /// Spatial satisfaction on a multiset of components.
    fn sp_multi(&mut self, s: &Sp, cs: &[ProcId]) -> bool {
        match cs.len() {
            0 => {
                let z = self.arena.nil();
                return self.sp(s, z);
            }
            1 if !matches!(s, Sp::Par(..)) => return self.sp(s, cs[0]),
            _ => {}
        }
        match s {
            Sp::True => true,
            Sp::Zero | Sp::Out { .. } | Sp::In { .. } => false,
            Sp::Not(a) => !self.sp_multi(a, cs),
            Sp::And(xs) => xs.iter().all(|x| self.sp_multi(x, cs)),
            Sp::Or(xs) => xs.iter().any(|x| self.sp_multi(x, cs)),
            Sp::Par(a, b) => {
                let n = cs.len();
                if n >= 63 || (1u64 << n) > self.max_splits {
                    self.error.get_or_insert(format!(
                        "separating conjunction over {n} components exceeds the split bound {}",
                        self.max_splits
                    ));
                    return false;
                }
                for mask in 0u64..(1u64 << n) {
                    let (l, r): (Vec<_>, Vec<_>) =
                        cs.iter().enumerate().partition(|(i, _)| mask & (1 << i) != 0);
                    let l: Vec<ProcId> = l.into_iter().map(|x| *x.1).collect();
                    let r: Vec<ProcId> = r.into_iter().map(|x| *x.1).collect();
                    if self.sp_multi(a, &l) && self.sp_multi(b, &r) {
                        return true;
                    }
                }
                false
            }
        }
    }
}
