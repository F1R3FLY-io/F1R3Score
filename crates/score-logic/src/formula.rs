//! The formula ASTs: spatial formulae over processes, crisp formulae over
//! candidates, configuration formulae (the behavioural part), and graded
//! clauses valued in (R>=0, +, x, 0, 1) -- Paper II's two sorts with the atoms
//! of Definition 3.1 of the note.

use score_core::{Datum, Timbre, Q};
use std::collections::BTreeMap;

/// A pattern for a name `<n, tau, d>`: `None` components are `_`.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct NamePat {
    /// a spatial formula on the quoted location, or `_`
    pub quote: Option<Box<Sp>>,
    pub timbre: Option<Timbre>,
    pub datum: Option<Datum>,
}

impl NamePat {
    pub fn any() -> NamePat {
        NamePat { quote: None, timbre: None, datum: None }
    }
}

/// Spatial formulae over processes.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Sp {
    True,
    /// the empty process
    Zero,
    Not(Box<Sp>),
    And(Vec<Sp>),
    Or(Vec<Sp>),
    /// separating conjunction: the multiset of components splits in two
    Par(Box<Sp>, Box<Sp>),
    /// `out`: a single message `<n,tau,d>!(payload, ptimbre, carry)`
    Out { subj: NamePat, payload: Box<Sp>, ptimbre: Option<Timbre>, carry: Option<Datum> },
    /// `in`: a single receipt `for(<n,tau,d>) body`
    In { subj: NamePat, body: Box<Sp> },
}

/// Atoms over a candidate's view. The `u8` is the chord slot (0 for a plain
/// receipt; `pitch#i` is slot `i-1`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Atom {
    Pitch(u8, Datum),
    Dur(u8, Datum),
    Carry(u8, Datum),
    /// carry and pitch are ordered pitches and ord(carry) - ord(pitch) = k
    Step(u8, i64),
    /// the location's quote satisfies the formula
    At(Sp),
    /// the passed process satisfies the formula
    Passes(u8, Sp),
}

/// Crisp formulae over candidates.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Crisp {
    True,
    False,
    Not(Box<Crisp>),
    And(Vec<Crisp>),
    Or(Vec<Crisp>),
    Atom(Atom),
    /// the residue of firing the candidate, in the present configuration,
    /// satisfies the configuration formula
    Leads(Box<Cfg>),
}

/// Configuration formulae: the context-labelled behavioural modality with
/// the actual company as its context (Decision "the actual company").
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Cfg {
    True,
    False,
    Not(Box<Cfg>),
    And(Vec<Cfg>),
    Or(Vec<Cfg>),
    /// some available candidate exists
    Live,
    /// `<g> phi`: some available candidate satisfying the crisp guard `g`
    /// (any candidate when absent) leads to a configuration satisfying phi
    Dia { guard: Option<Box<Crisp>>, body: Box<Cfg> },
    /// greatest fixed point; variables are numbered per clause
    Nu(u32, Box<Cfg>),
    Var(u32),
}

/// A function of the view used as a table key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum KeyFn {
    Pitch(u8),
    Dur(u8),
    Carry(u8),
    /// the datum on the subject of the location's record (the last duration
    /// for a pitch-leads voice)
    Prev,
    /// the datum the location's record carries (the held pitch)
    Held,
    Step(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum KeyVal {
    D(Datum),
    I(i64),
    /// the key function is undefined on this view
    Undef,
    /// unused position of a key shorter than three
    Pad,
}

pub type Key3 = [KeyVal; 3];

/// `table key { k: w, ..., _: w }`: a sum over mutually exclusive crisp
/// conjunctions, evaluated by lookup.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Table {
    pub key: Vec<KeyFn>,
    pub entries: BTreeMap<Key3, Q>,
    pub default: Q,
}

/// Graded clauses.
#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Graded {
    Const(Q),
    Crisp(Crisp),
    Tensor(Vec<Graded>),
    Sum(Vec<Graded>),
    Table(Table),
}

// ------------------------------------------------------------------- depth

impl Sp {
    /// The depth grading of the rho-life notes: one per `out`/`in`.
    pub fn depth(&self) -> u32 {
        match self {
            Sp::True | Sp::Zero => 0,
            Sp::Not(a) => a.depth(),
            Sp::And(v) | Sp::Or(v) => v.iter().map(|x| x.depth()).max().unwrap_or(0),
            Sp::Par(a, b) => a.depth().max(b.depth()),
            Sp::Out { subj, payload, .. } => 1 + payload.depth().max(subj.depth()),
            Sp::In { subj, body } => 1 + body.depth().max(subj.depth()),
        }
    }
}
impl NamePat {
    pub fn depth(&self) -> u32 {
        self.quote.as_ref().map(|q| q.depth()).unwrap_or(0)
    }
}
impl Atom {
    pub fn depth(&self) -> u32 {
        match self {
            Atom::At(s) | Atom::Passes(_, s) => s.depth(),
            _ => 0,
        }
    }
}
impl Crisp {
    pub fn depth(&self) -> u32 {
        match self {
            Crisp::True | Crisp::False => 0,
            Crisp::Not(a) => a.depth(),
            Crisp::And(v) | Crisp::Or(v) => v.iter().map(|x| x.depth()).max().unwrap_or(0),
            Crisp::Atom(a) => a.depth(),
            Crisp::Leads(c) => c.depth(),
        }
    }
    pub fn is_behavioural(&self) -> bool {
        match self {
            Crisp::True | Crisp::False | Crisp::Atom(_) => false,
            Crisp::Not(a) => a.is_behavioural(),
            Crisp::And(v) | Crisp::Or(v) => v.iter().any(|x| x.is_behavioural()),
            Crisp::Leads(_) => true,
        }
    }
}
impl Cfg {
    pub fn depth(&self) -> u32 {
        match self {
            Cfg::True | Cfg::False | Cfg::Live | Cfg::Var(_) => 0,
            Cfg::Not(a) | Cfg::Nu(_, a) => a.depth(),
            Cfg::And(v) | Cfg::Or(v) => v.iter().map(|x| x.depth()).max().unwrap_or(0),
            Cfg::Dia { guard, body } => guard.as_ref().map(|g| g.depth()).unwrap_or(0).max(body.depth()),
        }
    }
}
impl Graded {
    pub fn depth(&self) -> u32 {
        match self {
            Graded::Const(_) => 0,
            Graded::Crisp(c) => c.depth(),
            Graded::Tensor(v) | Graded::Sum(v) => v.iter().map(|x| x.depth()).max().unwrap_or(0),
            Graded::Table(t) => {
                if t.key.iter().any(|k| matches!(k, KeyFn::Prev | KeyFn::Held)) {
                    1
                } else {
                    0
                }
            }
        }
    }
    pub fn is_behavioural(&self) -> bool {
        match self {
            Graded::Const(_) | Graded::Table(_) => false,
            Graded::Crisp(c) => c.is_behavioural(),
            Graded::Tensor(v) | Graded::Sum(v) => v.iter().any(|x| x.is_behavioural()),
        }
    }
}

// ------------------------------------------------------------ derived atoms

/// `held(t) := at(<_,_,_>!(true, _, t))`
pub fn held(t: Datum) -> Crisp {
    Crisp::Atom(Atom::At(out_carry(Sp::True, Some(t))))
}
/// `prev(u) := at(<_,_,u>!true)`
pub fn prev(u: Datum) -> Crisp {
    Crisp::Atom(Atom::At(Sp::Out {
        subj: NamePat { quote: None, timbre: None, datum: Some(u) },
        payload: Box::new(Sp::True),
        ptimbre: None,
        carry: None,
    }))
}
fn out_carry(payload: Sp, carry: Option<Datum>) -> Sp {
    Sp::Out { subj: NamePat::any(), payload: Box::new(payload), ptimbre: None, carry }
}
/// `back(j, e)`: the record `j` levels into the past carries `e`.
pub fn back(j: u32, e: Datum) -> Crisp {
    let mut s = out_carry(Sp::True, Some(e));
    for _ in 0..j {
        s = out_carry(s, None);
    }
    Crisp::Atom(Atom::At(s))
}
/// `Last(m)` of the note, as a spatial formula: the most recent carried data,
/// ending with the outermost, are `m`.
pub fn last_formula(m: &[Datum]) -> Sp {
    let mut s = Sp::True;
    for &a in m {
        s = out_carry(s, Some(a));
    }
    s
}
/// `last[m1 .. mk] := at(Last(m))`
pub fn last(m: &[Datum]) -> Crisp {
    Crisp::Atom(Atom::At(last_formula(m)))
}
/// `call[m1 .. mk]`: `last` skipping the outermost record (the pitch a handoff
/// carries but never plays, Proposition 6.5).
pub fn call(m: &[Datum]) -> Crisp {
    Crisp::Atom(Atom::At(out_carry(last_formula(m), None)))
}

impl KeyFn {
    /// The crisp atom `k = v` whose disjunction the table's lookup realises.
    pub fn atom(&self, v: KeyVal) -> Crisp {
        match (self, v) {
            (KeyFn::Pitch(s), KeyVal::D(d)) => Crisp::Atom(Atom::Pitch(*s, d)),
            (KeyFn::Dur(s), KeyVal::D(d)) => Crisp::Atom(Atom::Dur(*s, d)),
            (KeyFn::Carry(s), KeyVal::D(d)) => Crisp::Atom(Atom::Carry(*s, d)),
            (KeyFn::Prev, KeyVal::D(d)) => prev(d),
            (KeyFn::Held, KeyVal::D(d)) => held(d),
            (KeyFn::Step(s), KeyVal::I(k)) => Crisp::Atom(Atom::Step(*s, k)),
            _ => Crisp::False,
        }
    }
}

impl Table {
    /// The same factor written term by term: `(+)_k [key = k] (x) w_k`
    /// plus the default on the complement.
    pub fn to_formula(&self) -> Graded {
        let mut terms = vec![];
        let mut conds = vec![];
        for (k, w) in &self.entries {
            let c = Crisp::And(self.key.iter().zip(k.iter()).map(|(f, v)| f.atom(*v)).collect());
            conds.push(c.clone());
            terms.push(Graded::Tensor(vec![Graded::Crisp(c), Graded::Const(w.clone())]));
        }
        if self.default.is_positive() {
            terms.push(Graded::Tensor(vec![
                Graded::Crisp(Crisp::Not(Box::new(Crisp::Or(conds)))),
                Graded::Const(self.default.clone()),
            ]));
        }
        Graded::Sum(terms)
    }
}
