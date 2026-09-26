//! The surface AST. Definitions' parameter sorts are known at parse time, so
//! application arguments are parsed at their sorts.

use crate::lexer::Span;
use score_core::Q;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Id {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    Proc,
    Name,
    Timbre,
    Pitch,
    Dur,
    Clause,
    Cont,
    Str,
    /// a finite set of pitches: `{A2, E2}`, `pitches`, `pitches-r`, or a
    /// declared `pitchset`
    Pitchset,
    /// `[S]`: a finite list
    List(Box<Sort>),
    /// `(S1, S2, ...)`: a tuple
    Tuple(Vec<Sort>),
}
impl Sort {
    pub fn parse(s: &str) -> Option<Sort> {
        Some(match s {
            "proc" => Sort::Proc,
            "name" => Sort::Name,
            "timbre" => Sort::Timbre,
            "pitch" => Sort::Pitch,
            "dur" => Sort::Dur,
            "clause" => Sort::Clause,
            "cont" => Sort::Cont,
            "string" => Sort::Str,
            "pitchset" => Sort::Pitchset,
            _ => return None,
        })
    }
    pub fn show(&self) -> String {
        match self {
            Sort::List(s) => format!("[{}]", s.show()),
            Sort::Tuple(v) => format!("({})", v.iter().map(|x| x.show()).collect::<Vec<_>>().join(", ")),
            s => s.as_str().to_string(),
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Sort::Pitchset => "pitchset",
            Sort::List(_) => "list",
            Sort::Tuple(_) => "tuple",
            Sort::Proc => "proc",
            Sort::Name => "name",
            Sort::Timbre => "timbre",
            Sort::Pitch => "pitch",
            Sort::Dur => "dur",
            Sort::Clause => "clause",
            Sort::Cont => "cont",
            Sort::Str => "string",
        }
    }
}

/// A decoration component as written: a name, or the wildcard `_`.
#[derive(Clone, Debug)]
pub enum DecoId {
    Id(Id),
    Wild(Span),
}

#[derive(Clone, Debug)]
pub enum PExpr {
    Zero,
    Par(Vec<PExpr>),
    For { label: Option<Id>, binds: Vec<(Option<Id>, NExpr)>, clause: Option<CExpr>, body: Box<PExpr>, span: Span },
    /// `x!(Q, t, d)`, or `x!(Q)` (both decorations `_`, `deco: None`)
    Send { subj: NExpr, payload: Box<PExpr>, deco: Option<(DecoId, DecoId)>, span: Span },
    /// synchronous output `step ; rest` (Def. 9.3): the step is one message
    /// or a parenthesised parallel group of messages
    Seq { step: Vec<PExpr>, rest: Box<PExpr>, span: Span },
    /// `keyloc(P, k)`: the key location K_{P,k}
    KeyLoc { p: Box<PExpr>, k: DecoId, span: Span },
    /// `keycode(P, k, tau)`: the key's code location C_{P,k,tau}
    KeyCode { p: Box<PExpr>, k: DecoId, tau: DecoId, span: Span },
    /// `ackloc(n)`: the n-th acknowledgement location; printed by `expand`
    /// so that an elaborated `;` re-parses, not meant to be written
    AckLoc(u64),
    Drop(NExpr),
    Ref(Id),
    App { def: Id, args: Vec<Arg>, label: Option<Id> },
    ContApp { param: Id, name: NExpr },
    ParComp { gens: Vec<Gen>, body: Box<PExpr> },
    Line { loc: Box<PExpr>, timbre: Id, notes: Vec<(Id, Id)>, then: Option<Box<PExpr>> },
    Base(SExpr),
    At { t: Q, body: Box<PExpr>, span: Span },
}

#[derive(Clone, Debug)]
pub enum NExpr {
    Ident(Id),
    /// `<@P, t, d>`; `@P` is `<@P, _, _>`
    Quote { proc: Box<PExpr>, timbre: DecoId, datum: DecoId },
    /// application of a definition that returns a name (`touch(P, k, d)`)
    App { def: Id, args: Vec<Arg> },
}

#[derive(Clone, Debug)]
pub enum SExpr {
    Lit(String),
    Ref(Id),
    Concat(Box<SExpr>, Box<SExpr>),
}

#[derive(Clone, Debug)]
pub enum Arg {
    Proc(PExpr),
    Name(NExpr),
    Timbre(DecoId),
    Datum(DecoId),
    Clause(CExpr),
    Str(SExpr),
    Cont(ContExpr),
    Pitchset(PSet),
    List(Vec<Arg>, Span),
    Tuple(Vec<Arg>, Span),
    /// the hole of a continuation (a name argument written `_`)
    Hole(Span),
}

/// A pitch set as written.
#[derive(Clone, Debug)]
pub enum PSet {
    /// `{A2, E2, ...}`
    Lit(Vec<Id>),
    /// `pitches`, `pitches-r`, a declared `pitchset`, or a parameter
    Ref(Id),
    Pitches,
    PitchesR,
}

#[derive(Clone, Debug)]
pub struct ContExpr {
    pub def: Id,
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug)]
pub enum GenSet {
    Pitches,
    PitchesR,
    Durations,
    DurationsPlus,
    List(Vec<Id>),
    /// a pitch set or list parameter, or a declared pitch set
    Ref(Id),
}

/// `x in S` or `(x, y) in S` (a tuple pattern over a list of tuples)
#[derive(Clone, Debug)]
pub struct Gen {
    pub vars: Vec<Id>,
    pub tuple: bool,
    pub set: GenSet,
}

#[derive(Clone, Debug)]
pub enum CExpr {
    Num(Q),
    Crisp(BExpr),
    Mul(Vec<CExpr>),
    Add(Vec<CExpr>),
    Ref(Id),
    Table { key: Vec<(Id, u8)>, entries: Vec<(Vec<KeyPat>, Q)>, default: Option<Q>, span: Span },
    Machine { pitch: bool, dual: bool, body: MachineBody, span: Span },
    SumComp { gens: Vec<Gen>, body: Box<CExpr> },
}

#[derive(Clone, Debug)]
pub enum MachineBody {
    Edges(Vec<(Id, Id, Q)>),
    Uniform,
    /// imported canonical machine: state count and indexed arrows
    Indexed { n: usize, edges: Vec<(usize, usize, Q)> },
}

#[derive(Clone, Debug)]
pub enum KeyPat {
    Datum(Id),
    Int(i64),
    Range(i64, i64),
}

#[derive(Clone, Debug)]
pub enum BExpr {
    True,
    False,
    Not(Box<BExpr>),
    And(Vec<BExpr>),
    Or(Vec<BExpr>),
    Atom(AtomAst),
    Leads(CfgAst),
    Any { gens: Vec<Gen>, body: Box<BExpr> },
    All { gens: Vec<Gen>, body: Box<BExpr> },
}

#[derive(Clone, Debug)]
pub enum AtomAst {
    Pitch(u8, Id),
    Dur(u8, Id),
    Carry(u8, DecoId),
    Step(u8, i64),
    At(SpAst),
    Passes(u8, SpAst),
    Held(Id),
    Prev(Id),
    Back(u32, Id),
    Last(Vec<Id>),
    Call(Vec<Id>),
}

#[derive(Clone, Debug)]
pub enum SpAst {
    True,
    Zero,
    Not(Box<SpAst>),
    And(Vec<SpAst>),
    Or(Vec<SpAst>),
    Par(Box<SpAst>, Box<SpAst>),
    Out { subj: NPat, payload: Box<SpAst>, ptimbre: PatId, carry: PatId },
    In { subj: NPat, body: Box<SpAst> },
}

/// A decoration pattern: `?` (anything), `_` (only a wildcard), or a name.
#[derive(Clone, Debug)]
pub enum PatId {
    Any,
    Wild,
    Id(Id),
}

#[derive(Clone, Debug)]
pub struct NPat {
    /// `None` is `?`
    pub quote: Option<Box<SpAst>>,
    pub timbre: PatId,
    pub datum: PatId,
}

#[derive(Clone, Debug)]
pub enum CfgAst {
    True,
    False,
    Live,
    Not(Box<CfgAst>),
    And(Vec<CfgAst>),
    Or(Vec<CfgAst>),
    Dia { guard: Option<Box<BExpr>>, body: Box<CfgAst> },
    Nu(Id, Box<CfgAst>),
    Var(Id),
}

#[derive(Clone, Debug)]
pub enum PitchEntry {
    Named(Id, Option<u8>),
    Scale { kind: String, lo: Id, hi: Id },
    /// `piano88`: A0 .. C8
    Piano88(Span),
}

#[derive(Clone, Debug)]
pub struct TimbreEntry {
    pub name: Id,
    pub program: Option<u8>,
    pub channel: Option<u8>,
}

#[derive(Clone, Debug)]
pub enum DefBody {
    Proc(PExpr),
    /// `def f(...): name = <...>`
    Name(NExpr),
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: Id,
    pub params: Vec<(Id, Sort)>,
    pub body: DefBody,
    pub file: String,
}

#[derive(Clone, Debug, Default)]
pub struct File {
    pub name: Option<String>,
    pub pitches: Option<(Vec<PitchEntry>, Span)>,
    pub durations: Option<(Vec<(Id, Q)>, Span)>,
    pub timbres: Option<(Vec<TimbreEntry>, Span)>,
    pub defs: Vec<Def>,
    pub factors: Vec<(Id, CExpr)>,
    /// `pitchset NAME = { ... }`
    pub pitchsets: Vec<(Id, Vec<Id>)>,
    pub play: Option<(PExpr, Span)>,
    pub file: String,
}
