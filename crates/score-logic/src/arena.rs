//! Interned clauses, the checkable fragment as a type, and evaluation plans.

use crate::formula::*;
use score_core::digest::{ClauseDigests, Digest};
use score_core::sha256::sha256;
use score_core::ClauseId;
use std::collections::HashMap;

/// Why a clause is outside the checkable fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhyNot(pub String);

impl std::fmt::Display for WhyNot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The two admitted classes (R-fragment).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Locality {
    /// no behavioural modality: the value depends only on the view
    Local,
    /// behavioural modalities and fixed points over the window of the past
    Behavioural,
}

/// A clause admitted to the checkable fragment. The only way to obtain one is
/// `ClauseArena::intern`, which runs the membership check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checked {
    pub id: ClauseId,
    pub class: Locality,
    pub depth: u32,
}

/// Which parts of a view a factor reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub pitch: bool,
    pub dur: bool,
    pub carry: bool,
    pub passes: bool,
    pub loc: bool,
    pub behaviour: bool,
    /// reads a chord slot other than the first
    pub other_slots: bool,
}

impl Reads {
    fn or(self, o: Reads) -> Reads {
        Reads {
            pitch: self.pitch || o.pitch,
            dur: self.dur || o.dur,
            carry: self.carry || o.carry,
            passes: self.passes || o.passes,
            loc: self.loc || o.loc,
            behaviour: self.behaviour || o.behaviour,
            other_slots: self.other_slots || o.other_slots,
        }
    }
}

pub struct ClauseArena {
    clauses: Vec<Graded>,
    plans: Vec<Graded>,
    info: Vec<Checked>,
    digests: Vec<Digest>,
    map: HashMap<Graded, ClauseId>,
}

impl Default for ClauseArena {
    fn default() -> Self {
        Self::new()
    }
}

fn cost_g(g: &Graded) -> u32 {
    match g {
        Graded::Const(_) => 0,
        Graded::Crisp(c) => cost_c(c),
        Graded::Table(_) => 2,
        Graded::Tensor(v) | Graded::Sum(v) => v.iter().map(cost_g).max().unwrap_or(0).max(2),
    }
}
fn cost_c(c: &Crisp) -> u32 {
    match c {
        Crisp::True | Crisp::False => 0,
        Crisp::Atom(Atom::At(_)) | Crisp::Atom(Atom::Passes(..)) => 1,
        Crisp::Atom(_) => 0,
        Crisp::Not(a) => cost_c(a),
        Crisp::And(v) | Crisp::Or(v) => v.iter().map(cost_c).max().unwrap_or(0),
        Crisp::Leads(_) => 100,
    }
}

/// Reorder commutative operands so cheap, local factors are evaluated first;
/// the value is unchanged (products and conjunctions commute), and a zero or
/// a false short-circuits before any behavioural formula is consulted.
fn plan_g(g: &Graded) -> Graded {
    match g {
        Graded::Tensor(v) => {
            let mut w: Vec<Graded> = v.iter().map(plan_g).collect();
            w.sort_by_key(cost_g);
            Graded::Tensor(w)
        }
        Graded::Sum(v) => Graded::Sum(v.iter().map(plan_g).collect()),
        Graded::Crisp(c) => Graded::Crisp(plan_c(c)),
        x => x.clone(),
    }
}
fn plan_c(c: &Crisp) -> Crisp {
    match c {
        Crisp::And(v) => {
            let mut w: Vec<Crisp> = v.iter().map(plan_c).collect();
            w.sort_by_key(cost_c);
            Crisp::And(w)
        }
        Crisp::Or(v) => {
            let mut w: Vec<Crisp> = v.iter().map(plan_c).collect();
            w.sort_by_key(cost_c);
            Crisp::Or(w)
        }
        Crisp::Not(a) => Crisp::Not(Box::new(plan_c(a))),
        x => x.clone(),
    }
}

/// Check a configuration formula: variables bound, and every occurrence of a
/// fixed-point variable positive (under an even number of negations).
fn check_cfg(c: &Cfg, bound: &mut Vec<(u32, bool)>, positive: bool) -> Result<(), WhyNot> {
    match c {
        Cfg::True | Cfg::False | Cfg::Live => Ok(()),
        Cfg::Not(a) => check_cfg(a, bound, !positive),
        Cfg::And(v) | Cfg::Or(v) => v.iter().try_for_each(|x| check_cfg(x, bound, positive)),
        Cfg::Dia { guard, body } => {
            if let Some(g) = guard {
                if g.is_behavioural() {
                    return Err(WhyNot(
                        "the guard of a behavioural modality must be a local crisp formula".into(),
                    ));
                }
            }
            check_cfg(body, bound, positive)
        }
        Cfg::Nu(x, body) => {
            bound.push((*x, positive));
            let r = check_cfg(body, bound, positive);
            bound.pop();
            r
        }
        Cfg::Var(x) => match bound.iter().rev().find(|(y, _)| y == x) {
            None => Err(WhyNot(format!("fixed-point variable #{x} is unbound"))),
            Some((_, pol)) if *pol != positive => Err(WhyNot(
                "a fixed-point variable occurs under a negation (not monotone)".into(),
            )),
            _ => Ok(()),
        },
    }
}
fn check_crisp(c: &Crisp) -> Result<(), WhyNot> {
    match c {
        Crisp::Not(a) => check_crisp(a),
        Crisp::And(v) | Crisp::Or(v) => v.iter().try_for_each(check_crisp),
        Crisp::Leads(cfg) => check_cfg(cfg, &mut vec![], true),
        _ => Ok(()),
    }
}
fn check_graded(g: &Graded) -> Result<(), WhyNot> {
    match g {
        Graded::Crisp(c) => check_crisp(c),
        Graded::Tensor(v) | Graded::Sum(v) => v.iter().try_for_each(check_graded),
        Graded::Table(t) => {
            if t.key.is_empty() || t.key.len() > 3 {
                return Err(WhyNot("a table key has one to three components".into()));
            }
            Ok(())
        }
        Graded::Const(_) => Ok(()),
    }
}

pub fn reads_graded(g: &Graded) -> Reads {
    match g {
        Graded::Const(_) => Reads::default(),
        Graded::Crisp(c) => reads_crisp(c),
        Graded::Tensor(v) | Graded::Sum(v) => v.iter().fold(Reads::default(), |a, x| a.or(reads_graded(x))),
        Graded::Table(t) => t.key.iter().fold(Reads::default(), |a, k| {
            a.or(match k {
                KeyFn::Pitch(s) => Reads { pitch: true, other_slots: *s != 0, ..Default::default() },
                KeyFn::Dur(s) => Reads { dur: true, other_slots: *s != 0, ..Default::default() },
                KeyFn::Carry(s) => Reads { carry: true, other_slots: *s != 0, ..Default::default() },
                KeyFn::Step(s) => Reads { pitch: true, carry: true, other_slots: *s != 0, ..Default::default() },
                KeyFn::Prev | KeyFn::Held => Reads { loc: true, ..Default::default() },
            })
        }),
    }
}
fn reads_crisp(c: &Crisp) -> Reads {
    match c {
        Crisp::True | Crisp::False => Reads::default(),
        Crisp::Not(a) => reads_crisp(a),
        Crisp::And(v) | Crisp::Or(v) => v.iter().fold(Reads::default(), |a, x| a.or(reads_crisp(x))),
        Crisp::Leads(_) => Reads { behaviour: true, ..Default::default() },
        Crisp::Atom(a) => match a {
            Atom::Pitch(s, _) => Reads { pitch: true, other_slots: *s != 0, ..Default::default() },
            Atom::Dur(s, _) => Reads { dur: true, other_slots: *s != 0, ..Default::default() },
            Atom::Carry(s, _) => Reads { carry: true, other_slots: *s != 0, ..Default::default() },
            Atom::Step(s, _) => Reads { pitch: true, carry: true, other_slots: *s != 0, ..Default::default() },
            Atom::At(_) => Reads { loc: true, ..Default::default() },
            Atom::Passes(s, _) => Reads { passes: true, other_slots: *s != 0, ..Default::default() },
        },
    }
}

impl ClauseArena {
    pub fn new() -> Self {
        let mut a = ClauseArena { clauses: vec![], plans: vec![], info: vec![], digests: vec![], map: HashMap::new() };
        let t = a.intern(Graded::Crisp(Crisp::True)).expect("true is checkable");
        assert_eq!(t.id, ClauseId::TRUE);
        a
    }

    /// Intern a clause, admitting it to the checkable fragment or saying why not.
    pub fn intern(&mut self, g: Graded) -> Result<Checked, WhyNot> {
        if let Some(&id) = self.map.get(&g) {
            return Ok(self.info[id.0 as usize]);
        }
        check_graded(&g)?;
        let id = ClauseId(self.clauses.len() as u32);
        let class = if g.is_behavioural() { Locality::Behavioural } else { Locality::Local };
        let info = Checked { id, class, depth: g.depth() };
        self.plans.push(plan_g(&g));
        self.digests.push(sha256(format!("{g:?}").as_bytes()));
        self.clauses.push(g.clone());
        self.info.push(info);
        self.map.insert(g, id);
        Ok(info)
    }
    pub fn get(&self, c: ClauseId) -> &Graded {
        &self.clauses[c.0 as usize]
    }
    /// The evaluation plan: the same clause with cheap factors first.
    pub fn plan(&self, c: ClauseId) -> &Graded {
        &self.plans[c.0 as usize]
    }
    pub fn info(&self, c: ClauseId) -> Checked {
        self.info[c.0 as usize]
    }
    pub fn len(&self) -> usize {
        self.clauses.len()
    }
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }
    /// The top-level product factors of a clause with what each reads.
    pub fn factors(&self, c: ClauseId) -> Vec<(Graded, Reads)> {
        match self.get(c) {
            Graded::Tensor(v) => v.iter().map(|g| (g.clone(), reads_graded(g))).collect(),
            g => vec![(g.clone(), reads_graded(g))],
        }
    }
}

impl ClauseDigests for ClauseArena {
    fn clause_digest(&self, c: ClauseId) -> Digest {
        self.digests[c.0 as usize]
    }
}
