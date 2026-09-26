//! The surface AST. Definitions' parameter sorts are known at parse time, so
//! application arguments are parsed at their sorts.

use crate::lexer::Span;
use score_core::Q;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Id {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    Proc,
    Name,
    Timbre,
    Pitch,
    Dur,
    Clause,
    Cont,
    Str,
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
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
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

#[derive(Clone, Debug)]
pub enum PExpr {
    Zero,
    Par(Vec<PExpr>),
    For { label: Option<Id>, binds: Vec<(Option<Id>, NExpr)>, clause: Option<CExpr>, body: Box<PExpr>, span: Span },
    Send { subj: NExpr, payload: Box<PExpr>, timbre: Id, datum: Id, span: Span },
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
    Quote { proc: Box<PExpr>, timbre: Id, datum: Id },
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
    Timbre(Id),
    Datum(Id),
    Clause(CExpr),
    Str(SExpr),
    Cont(ContExpr),
    Hole(Span),
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
}

#[derive(Clone, Debug)]
pub struct Gen {
    pub var: Id,
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
    Carry(u8, Id),
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
    Out { subj: NPat, payload: Box<SpAst>, ptimbre: Option<Id>, carry: Option<Id> },
    In { subj: NPat, body: Box<SpAst> },
}

#[derive(Clone, Debug)]
pub struct NPat {
    pub quote: Option<Box<SpAst>>,
    pub timbre: Option<Id>,
    pub datum: Option<Id>,
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
}

#[derive(Clone, Debug)]
pub struct TimbreEntry {
    pub name: Id,
    pub program: Option<u8>,
    pub channel: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: Id,
    pub params: Vec<(Id, Sort)>,
    pub body: PExpr,
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
    pub play: Option<(PExpr, Span)>,
    pub file: String,
}
