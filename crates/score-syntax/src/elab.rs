//! Elaboration: surface AST -> closed core terms (R-macros).
//!
//! Definitions are macros expanded at elaboration time. Bound names are
//! de Bruijn levels while their binder is open and are closed into indices
//! when it closes, so expansion is capture-avoiding by construction. Expansion
//! is memoised on (definition, argument values): the result of expanding a
//! definition depends only on its arguments, which is what keeps unrolled
//! voices (a step whose every hand continues with the next step) linear in
//! size instead of exponential.

use crate::ast::*;
use crate::lexer::{Diag, Span};
use score_core::alphabet::{expand_scale, scientific_midi};
use score_core::base::base;
use score_core::digest::Digester;
use score_core::sha256::{hex, Sha256};
use score_core::*;
use score_logic::formula as f;
use score_logic::{ClauseArena, Crisp, Graded, KeyFn, KeyVal, Locality, Sp};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Val {
    Proc(ProcId),
    Name(Name),
    Timbre(Timbre),
    Datum(Datum),
    Clause(ClauseId),
    Str(String),
    Cont(Rc<ContVal>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ContVal {
    def: usize,
    /// exactly one `None`: the hole, of sort name
    args: Vec<Option<Val>>,
}

/// An elaborated score.
pub struct Score {
    pub name: Option<String>,
    pub alph: Alphabets,
    pub arena: Arena,
    pub clauses: ClauseArena,
    /// the initial configuration: stamped closed processes
    pub initial: Vec<(Q, ProcId)>,
    pub warnings: Vec<Diag>,
    pub digest: [u8; 32],
    /// the greatest depth of any spatial formula (the window of the past)
    pub window: u32,
    pub behavioural: bool,
}

impl Score {
    pub fn digest_hex(&self) -> String {
        hex(&self.digest)
    }
}

type R<T> = Result<T, Diag>;

struct Env {
    scope: Vec<(String, Val)>,
    level: u32,
    file: String,
    nu: Vec<(String, u32)>,
}
impl Env {
    fn get(&self, n: &str) -> Option<&Val> {
        self.scope.iter().rev().find(|(k, _)| k == n).map(|x| &x.1)
    }
}

struct Elab<'a> {
    arena: &'a mut Arena,
    clauses: &'a mut ClauseArena,
    alph: &'a Alphabets,
    defs: &'a [Def],
    index: HashMap<String, usize>,
    factors: HashMap<String, Graded>,
    memo: HashMap<(usize, Vec<Val>), ProcId>,
    warnings: Vec<Diag>,
    nu_counter: u32,
    behavioural: bool,
    window: u32,
}

fn diag(file: &str, span: Span, msg: impl Into<String>) -> Diag {
    Diag { file: file.into(), span, msg: msg.into() }
}

/// Build the alphabets from the declarations.
pub fn alphabets(file: &File) -> R<Alphabets> {
    let fname = &file.file;
    let (pents, psp) = file
        .pitches
        .clone()
        .ok_or_else(|| diag(fname, Span::default(), "a score must declare `pitches { r, ... }`"))?;
    let mut pitches = vec![];
    for e in pents {
        match e {
            PitchEntry::Named(id, midi) => {
                let m = midi.or_else(|| scientific_midi(&id.name));
                pitches.push(PitchDecl { name: id.name, midi: m });
            }
            PitchEntry::Scale { kind, lo, hi } => {
                pitches.extend(expand_scale(&kind, &lo.name, &hi.name).map_err(|e| diag(fname, lo.span, e))?);
            }
        }
    }
    let durations = file
        .durations
        .clone()
        .map(|(v, _)| v.into_iter().map(|(id, q)| DurDecl { name: id.name, len: q }).collect())
        .unwrap_or_default();
    let timbres: Vec<TimbreDecl> = file
        .timbres
        .clone()
        .map(|(v, _)| {
            v.into_iter()
                .enumerate()
                .map(|(i, t)| TimbreDecl {
                    name: t.name.name,
                    program: t.program,
                    channel: t.channel.unwrap_or(((i % 16) + 1) as u8),
                })
                .collect()
        })
        .unwrap_or_default();
    Alphabets::new(pitches, durations, timbres).map_err(|e| diag(fname, psp, e.0))
}

/// Elaborate a parsed file into a fresh arena.
pub fn elaborate(file: &File) -> R<Score> {
    let alph = alphabets(file)?;
    let mut arena = Arena::new();
    let mut clauses = ClauseArena::new();
    let (initial, warnings, window, behavioural) = elaborate_into(file, &alph, &mut arena, &mut clauses)?;
    let digest = score_digest(&alph, &arena, &clauses, &initial);
    Ok(Score { name: file.name.clone(), alph, arena, clauses, initial, warnings, digest, window, behavioural })
}

/// The digest of a score: alphabets and the initial configuration, id-independent.
pub fn score_digest(alph: &Alphabets, arena: &Arena, clauses: &ClauseArena, initial: &[(Q, ProcId)]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"f1r3score/1\n");
    for p in &alph.pitches {
        h.update(format!("p {} {:?}\n", p.name, p.midi).as_bytes());
    }
    for d in &alph.durations {
        h.update(format!("d {} {}\n", d.name, d.len).as_bytes());
    }
    for t in &alph.timbres {
        h.update(format!("t {} {:?} {}\n", t.name, t.program, t.channel).as_bytes());
    }
    let mut dg = Digester::new();
    let mut items: Vec<(Q, [u8; 32])> = initial.iter().map(|(t, p)| (t.clone(), dg.proc(arena, clauses, *p))).collect();
    items.sort();
    for (t, d) in items {
        h.update(format!("i {t} {}\n", hex(&d)).as_bytes());
    }
    h.finish()
}

/// Elaborate into an existing arena (so that terms can be compared by id).
#[allow(clippy::type_complexity)]
pub fn elaborate_into(
    file: &File,
    alph: &Alphabets,
    arena: &mut Arena,
    clauses: &mut ClauseArena,
) -> R<(Vec<(Q, ProcId)>, Vec<Diag>, u32, bool)> {
    let mut index = HashMap::new();
    for (i, d) in file.defs.iter().enumerate() {
        index.insert(d.name.name.clone(), i);
    }
    check_acyclic(&file.defs, &index)?;
    let mut e = Elab {
        arena,
        clauses,
        alph,
        defs: &file.defs,
        index,
        factors: HashMap::new(),
        memo: HashMap::new(),
        warnings: vec![],
        nu_counter: 0,
        behavioural: false,
        window: 0,
    };
    for (id, c) in &file.factors {
        let mut env = Env { scope: vec![], level: 0, file: file.file.clone(), nu: vec![] };
        let g = e.graded(c, &mut env)?;
        // admit the factor on its own, so its diagnostics point at it
        e.intern(g.clone(), &file.file, id.span)?;
        if e.factors.insert(id.name.clone(), g).is_some() {
            return Err(diag(&file.file, id.span, format!("factor `{}` defined twice", id.name)));
        }
    }
    let (play, psp) = file.play.clone().ok_or_else(|| diag(&file.file, Span::default(), "a score needs a `play` item"))?;
    let mut items = vec![];
    let mut spine = vec![play];
    while let Some(x) = spine.pop() {
        match x {
            PExpr::Par(v) => spine.extend(v.into_iter().rev()),
            PExpr::At { t, body, .. } => items.push((t, *body)),
            other => items.push((Q::ZERO, other)),
        }
    }
    // the initial configuration: one parallel composition per distinct stamp
    let mut by_stamp: BTreeMap<Q, Vec<ProcId>> = BTreeMap::new();
    for (t, x) in items {
        let mut env = Env { scope: vec![], level: 0, file: file.file.clone(), nu: vec![] };
        let p = e.proc(&x, &mut env)?;
        by_stamp.entry(t).or_default().push(p);
    }
    let initial: Vec<(Q, ProcId)> = by_stamp.into_iter().map(|(t, ps)| (t, e.arena.par(ps))).collect();
    lint_initial(e.arena, alph, &initial, &file.file, psp, &mut e.warnings);
    Ok((initial, e.warnings, e.window, e.behavioural))
}

fn check_acyclic(defs: &[Def], index: &HashMap<String, usize>) -> R<()> {
    fn refs_p(e: &PExpr, out: &mut Vec<(String, Span)>) {
        match e {
            PExpr::Par(v) => v.iter().for_each(|x| refs_p(x, out)),
            PExpr::For { binds, body, .. } => {
                binds.iter().for_each(|(_, n)| refs_n(n, out));
                refs_p(body, out)
            }
            PExpr::Send { subj, payload, .. } => {
                refs_n(subj, out);
                refs_p(payload, out)
            }
            PExpr::Drop(n) => refs_n(n, out),
            PExpr::App { def, args, .. } => {
                out.push((def.name.clone(), def.span));
                args.iter().for_each(|a| refs_a(a, out))
            }
            PExpr::ContApp { name, .. } => refs_n(name, out),
            PExpr::ParComp { body, .. } | PExpr::At { body, .. } => refs_p(body, out),
            PExpr::Line { loc, then, .. } => {
                refs_p(loc, out);
                if let Some(t) = then {
                    refs_p(t, out)
                }
            }
            _ => {}
        }
    }
    fn refs_n(n: &NExpr, out: &mut Vec<(String, Span)>) {
        if let NExpr::Quote { proc, .. } = n {
            refs_p(proc, out)
        }
    }
    fn refs_a(a: &Arg, out: &mut Vec<(String, Span)>) {
        match a {
            Arg::Proc(p) => refs_p(p, out),
            Arg::Name(n) => refs_n(n, out),
            Arg::Cont(c) => {
                out.push((c.def.name.clone(), c.def.span));
                c.args.iter().for_each(|a| refs_a(a, out))
            }
            _ => {}
        }
    }
    let n = defs.len();
    let mut edges = vec![vec![]; n];
    for (i, d) in defs.iter().enumerate() {
        let mut out = vec![];
        refs_p(&d.body, &mut out);
        for (r, sp) in out {
            if let Some(&j) = index.get(&r) {
                edges[i].push((j, sp));
            }
        }
    }
    // colour DFS
    let mut colour = vec![0u8; n];
    fn dfs(i: usize, edges: &[Vec<(usize, Span)>], colour: &mut [u8], defs: &[Def]) -> R<()> {
        colour[i] = 1;
        for &(j, sp) in &edges[i] {
            if colour[j] == 1 {
                return Err(diag(
                    &defs[i].file,
                    sp,
                    format!(
                        "recursive definition: `{}` refers to `{}`, which leads back to it. Definitions are \
                         macros and must form an acyclic call graph; unbounded behaviour is obtained by \
                         reflection, not by recursion (note, Section 4.2 and Remark 2.1: use a server that \
                         keeps its own code on a code channel, as `Voice` in std.score does)",
                        defs[i].name.name, defs[j].name.name
                    ),
                ));
            }
            if colour[j] == 0 {
                dfs(j, edges, colour, defs)?;
            }
        }
        colour[i] = 2;
        Ok(())
    }
    for i in 0..n {
        if colour[i] == 0 {
            dfs(i, &edges, &mut colour, defs)?;
        }
    }
    Ok(())
}

fn lint_initial(a: &Arena, alph: &Alphabets, initial: &[(Q, ProcId)], file: &str, sp: Span, w: &mut Vec<Diag>) {
    let mut at: HashMap<Loc, (HashSet<bool>, HashSet<bool>)> = HashMap::new();
    for (_, p) in initial {
        for c in a.components(*p) {
            match a.get(c) {
                Proc::Recv { subjects, .. } => {
                    for s in subjects.iter() {
                        if let Name::Quote { proc, timbre, datum } = s {
                            at.entry(Loc { quote: *proc, timbre: *timbre })
                                .or_default()
                                .0
                                .insert(alph.is_pitch(*datum));
                        }
                    }
                }
                Proc::Send { subj: Name::Quote { proc, timbre, datum }, .. } => {
                    at.entry(Loc { quote: *proc, timbre: *timbre }).or_default().1.insert(alph.is_pitch(*datum));
                }
                _ => {}
            }
        }
    }
    let mut locs: Vec<_> = at.into_iter().collect();
    locs.sort_by_key(|x| x.0);
    for (l, (r, m)) in locs {
        for pol in [true, false] {
            if r.contains(&pol) && m.contains(&pol) && !m.contains(&!pol) && !r.contains(&!pol) {
                w.push(diag(
                    file,
                    sp,
                    format!(
                        "warning: a top-level receipt and message of the same polarity ({}) share a location in \
                         timbre `{}`; they can never communicate (Proposition 2.4(ii))",
                        if pol { "channel" } else { "co-channel" },
                        alph.timbre_name(l.timbre)
                    ),
                ));
            }
        }
    }
}

impl<'a> Elab<'a> {
    fn intern(&mut self, g: Graded, file: &str, sp: Span) -> R<ClauseId> {
        match self.clauses.intern(g) {
            Ok(c) => {
                if c.class == Locality::Behavioural {
                    self.behavioural = true;
                }
                self.window = self.window.max(c.depth);
                Ok(c.id)
            }
            Err(w) => Err(diag(file, sp, format!("clause outside the checkable fragment: {w}"))),
        }
    }

    fn datum(&self, id: &Id, env: &Env, want: Option<Sort>) -> R<Datum> {
        let d = match env.get(&id.name) {
            Some(Val::Datum(d)) => *d,
            Some(v) => {
                return Err(diag(&env.file, id.span, format!("`{}` is a {}, not a datum", id.name, kind(v))))
            }
            None => self.alph.lookup_datum(&id.name).ok_or_else(|| {
                diag(&env.file, id.span, format!("`{}` is not a declared pitch or duration", id.name))
            })?,
        };
        match want {
            Some(Sort::Pitch) if !self.alph.is_pitch(d) => {
                Err(diag(&env.file, id.span, format!("`{}` is a duration where a pitch is expected", id.name)))
            }
            Some(Sort::Dur) if self.alph.is_pitch(d) => {
                Err(diag(&env.file, id.span, format!("`{}` is a pitch where a duration is expected", id.name)))
            }
            _ => Ok(d),
        }
    }

    fn timbre(&self, id: &Id, env: &Env) -> R<Timbre> {
        match env.get(&id.name) {
            Some(Val::Timbre(t)) => Ok(*t),
            Some(v) => Err(diag(&env.file, id.span, format!("`{}` is a {}, not a timbre", id.name, kind(v)))),
            None => self
                .alph
                .lookup_timbre(&id.name)
                .ok_or_else(|| diag(&env.file, id.span, format!("`{}` is not a declared timbre", id.name))),
        }
    }

    fn string(&self, s: &SExpr, env: &Env) -> R<String> {
        match s {
            SExpr::Lit(x) => Ok(x.clone()),
            SExpr::Ref(id) => match env.get(&id.name) {
                Some(Val::Str(x)) => Ok(x.clone()),
                _ => Err(diag(&env.file, id.span, format!("`{}` is not a string parameter", id.name))),
            },
            SExpr::Concat(a, b) => Ok(format!("{}{}", self.string(a, env)?, self.string(b, env)?)),
        }
    }

    fn name(&mut self, n: &NExpr, env: &mut Env) -> R<Name> {
        match n {
            NExpr::Ident(id) => match env.get(&id.name) {
                Some(Val::Name(x)) => Ok(*x),
                Some(v) => Err(diag(&env.file, id.span, format!("`{}` is a {}, not a name", id.name, kind(v)))),
                None => Err(diag(&env.file, id.span, format!("unbound name `{}`", id.name))),
            },
            NExpr::Quote { proc, timbre, datum } => {
                let p = self.proc(proc, env)?;
                let t = self.timbre(timbre, env)?;
                let d = self.datum(datum, env, None)?;
                Ok(Name::Quote { proc: p, timbre: t, datum: d })
            }
        }
    }

    fn gen_values(&self, g: &Gen, env: &Env) -> R<Vec<Datum>> {
        Ok(match &g.set {
            GenSet::Pitches => self.alph.all_pitches(),
            GenSet::PitchesR => self.alph.ordered_pitches().to_vec(),
            GenSet::Durations => self.alph.all_durations(),
            GenSet::DurationsPlus => self.alph.positive_durations().to_vec(),
            GenSet::List(v) => v.iter().map(|x| self.datum(x, env, None)).collect::<R<_>>()?,
        })
    }

    fn product(&self, gens: &[Gen], env: &Env) -> R<Vec<Vec<(String, Datum)>>> {
        let mut acc: Vec<Vec<(String, Datum)>> = vec![vec![]];
        for g in gens {
            let vals = self.gen_values(g, env)?;
            let mut next = vec![];
            for a in &acc {
                for v in &vals {
                    let mut b = a.clone();
                    b.push((g.var.name.clone(), *v));
                    next.push(b);
                }
            }
            acc = next;
        }
        Ok(acc)
    }

    // ---------------------------------------------------------------- procs

    fn proc(&mut self, e: &PExpr, env: &mut Env) -> R<ProcId> {
        match e {
            PExpr::Zero => Ok(self.arena.nil()),
            PExpr::Par(v) => {
                let mut ps = vec![];
                for x in v {
                    ps.push(self.proc(x, env)?);
                }
                Ok(self.arena.par(ps))
            }
            PExpr::For { label, binds, clause, body, span } => {
                let l = env.level;
                let mut subjects = vec![];
                for (_, n) in binds {
                    subjects.push(self.name(n, env)?);
                }
                let cid = match clause {
                    Some(c) => {
                        env.nu.clear();
                        self.nu_counter = 0;
                        let g = self.graded(c, env)?;
                        self.intern(g, &env.file.clone(), *span)?
                    }
                    None => ClauseId::TRUE,
                };
                let k = binds.len() as u32;
                let mark = env.scope.len();
                for (i, (b, _)) in binds.iter().enumerate() {
                    if let Some(b) = b {
                        env.scope.push((b.name.clone(), Val::Name(Name::Lvl(l + i as u32))));
                    }
                }
                env.level += k;
                let b = self.proc(body, env);
                env.scope.truncate(mark);
                env.level = l;
                let b = b?;
                let closed = self.arena.close(b, l, k);
                let lab = label.as_ref().map(|x| self.arena.label(&x.name));
                Ok(self.arena.recv(subjects, cid, closed, lab))
            }
            PExpr::Send { subj, payload, timbre, datum, span } => {
                let s = self.name(subj, env)?;
                let p = self.proc(payload, env)?;
                let t = self.timbre(timbre, env)?;
                let d = self.datum(datum, env, None)?;
                if let Name::Quote { timbre: st, .. } = s {
                    if st != t {
                        self.warnings.push(diag(
                            &env.file,
                            *span,
                            format!(
                                "warning: the payload timbre `{}` differs from the subject's timbre `{}`; this \
                                 message can never communicate",
                                self.alph.timbre_name(t),
                                self.alph.timbre_name(st)
                            ),
                        ));
                    }
                }
                Ok(self.arena.send(s, p, t, d))
            }
            PExpr::Drop(n) => {
                let n = self.name(n, env)?;
                Ok(self.arena.drop_name(n))
            }
            PExpr::Ref(id) => match env.get(&id.name) {
                Some(Val::Proc(p)) => Ok(*p),
                Some(v) => Err(diag(&env.file, id.span, format!("`{}` is a {}, not a process", id.name, kind(v)))),
                None => Err(diag(&env.file, id.span, format!("unknown process `{}`", id.name))),
            },
            PExpr::App { def, args, label } => {
                let di = *self
                    .index
                    .get(&def.name)
                    .ok_or_else(|| diag(&env.file, def.span, format!("unknown definition `{}`", def.name)))?;
                let params = self.defs[di].params.clone();
                let mut vals = vec![];
                for (a, (pid, s)) in args.iter().zip(params.iter()) {
                    vals.push(self.arg(a, *s, pid, env)?);
                }
                let p = self.expand(di, vals, env)?;
                Ok(match label {
                    Some(l) => {
                        let lab = self.arena.label(&l.name);
                        self.arena.relabel(p, lab)
                    }
                    None => p,
                })
            }
            PExpr::ContApp { param, name } => {
                let cv = match env.get(&param.name) {
                    Some(Val::Cont(c)) => c.clone(),
                    _ => return Err(diag(&env.file, param.span, format!("`{}` is not a continuation", param.name))),
                };
                let n = self.name(name, env)?;
                let vals: Vec<Val> =
                    cv.args.iter().map(|a| a.clone().unwrap_or(Val::Name(n))).collect();
                self.expand(cv.def, vals, env)
            }
            PExpr::ParComp { gens, body } => {
                let combos = self.product(gens, env)?;
                let mut ps = vec![];
                for c in combos {
                    let mark = env.scope.len();
                    for (k, d) in c {
                        env.scope.push((k, Val::Datum(d)));
                    }
                    let r = self.proc(body, env);
                    env.scope.truncate(mark);
                    ps.push(r?);
                }
                Ok(self.arena.par(ps))
            }
            PExpr::Line { loc, timbre, notes, then } => {
                let l = self.proc(loc, env)?;
                let t = self.timbre(timbre, env)?;
                let mut k = match then {
                    Some(x) => self.proc(x, env)?,
                    None => self.arena.nil(),
                };
                let rest = self.alph.rest();
                for (p, d) in notes.iter().rev() {
                    let pd = self.datum(p, env, Some(Sort::Pitch))?;
                    let dd = self.datum(d, env, Some(Sort::Dur))?;
                    let body = self.arena.close(k, env.level, 1);
                    let hand = self.arena.recv(vec![Name::Quote { proc: l, timbre: t, datum: pd }], ClauseId::TRUE, body, None);
                    let nil = self.arena.nil();
                    let msg = self.arena.send(Name::Quote { proc: l, timbre: t, datum: dd }, nil, t, rest);
                    k = self.arena.par([hand, msg]);
                }
                Ok(k)
            }
            PExpr::Base(s) => {
                let tag = self.string(s, env)?;
                Ok(base(self.arena, &tag, self.alph.rest()))
            }
            PExpr::At { span, .. } => Err(diag(
                &env.file,
                *span,
                "`at t { P }` stamps an entrance and is allowed only at the top level of `play`",
            )),
        }
    }

    fn expand(&mut self, di: usize, vals: Vec<Val>, env: &Env) -> R<ProcId> {
        let key = (di, vals);
        if let Some(&p) = self.memo.get(&key) {
            return Ok(p);
        }
        let d = &self.defs[di];
        let mut inner = Env {
            scope: d.params.iter().map(|(i, _)| i.name.clone()).zip(key.1.iter().cloned()).collect(),
            level: env.level,
            file: d.file.clone(),
            nu: vec![],
        };
        let body = d.body.clone();
        let p = self.proc(&body, &mut inner)?;
        self.memo.insert(key, p);
        Ok(p)
    }

    fn arg(&mut self, a: &Arg, s: Sort, pid: &Id, env: &mut Env) -> R<Val> {
        Ok(match (a, s) {
            (Arg::Proc(p), Sort::Proc) => Val::Proc(self.proc(p, env)?),
            (Arg::Name(n), Sort::Name) => Val::Name(self.name(n, env)?),
            (Arg::Timbre(t), Sort::Timbre) => Val::Timbre(self.timbre(t, env)?),
            (Arg::Datum(d), Sort::Pitch) | (Arg::Datum(d), Sort::Dur) => Val::Datum(self.datum(d, env, Some(s))?),
            (Arg::Clause(c), Sort::Clause) => {
                env.nu.clear();
                self.nu_counter = 0;
                let g = self.graded(c, env)?;
                Val::Clause(self.intern(g, &env.file.clone(), pid.span)?)
            }
            (Arg::Str(x), Sort::Str) => Val::Str(self.string(x, env)?),
            (Arg::Cont(c), Sort::Cont) => {
                if c.args.is_empty() {
                    if let Some(Val::Cont(v)) = env.get(&c.def.name) {
                        return Ok(Val::Cont(v.clone()));
                    }
                }
                let di = *self.index.get(&c.def.name).ok_or_else(|| {
                    diag(&env.file, c.def.span, format!("unknown definition `{}`", c.def.name))
                })?;
                let params = self.defs[di].params.clone();
                if c.args.len() != params.len() {
                    return Err(diag(&env.file, c.def.span, format!("`{}` takes {} arguments", c.def.name, params.len())));
                }
                let mut vals = vec![];
                let mut holes = 0;
                for (a, (q, qs)) in c.args.iter().zip(params.iter()) {
                    match a {
                        Arg::Hole(sp) => {
                            if *qs != Sort::Name {
                                return Err(diag(&env.file, *sp, "the hole of a continuation must be a name parameter"));
                            }
                            holes += 1;
                            vals.push(None);
                        }
                        a => vals.push(Some(self.arg(a, *qs, q, env)?)),
                    }
                }
                if holes != 1 {
                    return Err(diag(
                        &env.file,
                        c.def.span,
                        "a continuation is a definition applied to all but one name argument, written `_`",
                    ));
                }
                Val::Cont(Rc::new(ContVal { def: di, args: vals }))
            }
            (Arg::Hole(sp), _) => return Err(diag(&env.file, *sp, "`_` is only allowed in a continuation argument")),
            (_, s) => {
                return Err(diag(
                    &env.file,
                    pid.span,
                    format!("ill-sorted argument for parameter `{}` of sort {}", pid.name, s.as_str()),
                ))
            }
        })
    }

    // -------------------------------------------------------------- clauses

    fn graded(&mut self, c: &CExpr, env: &mut Env) -> R<Graded> {
        Ok(match c {
            CExpr::Num(q) => Graded::Const(q.clone()),
            CExpr::Crisp(b) => Graded::Crisp(self.crisp(b, env)?),
            CExpr::Mul(v) => {
                let mut out = vec![];
                for x in v {
                    match self.graded(x, env)? {
                        Graded::Tensor(w) => out.extend(w),
                        g => out.push(g),
                    }
                }
                Graded::Tensor(out)
            }
            CExpr::Add(v) => {
                let mut out = vec![];
                for x in v {
                    match self.graded(x, env)? {
                        Graded::Sum(w) => out.extend(w),
                        g => out.push(g),
                    }
                }
                Graded::Sum(out)
            }
            CExpr::Ref(id) => match env.get(&id.name) {
                Some(Val::Clause(c)) => self.clauses.get(*c).clone(),
                Some(v) => {
                    return Err(diag(&env.file, id.span, format!("`{}` is a {}, not a clause", id.name, kind(v))))
                }
                None => self
                    .factors
                    .get(&id.name)
                    .cloned()
                    .ok_or_else(|| diag(&env.file, id.span, format!("unknown factor or clause `{}`", id.name)))?,
            },
            CExpr::SumComp { gens, body } => {
                let combos = self.product(gens, env)?;
                let mut out = vec![];
                for cb in combos {
                    let mark = env.scope.len();
                    for (k, d) in cb {
                        env.scope.push((k, Val::Datum(d)));
                    }
                    let r = self.graded(body, env);
                    env.scope.truncate(mark);
                    out.push(r?);
                }
                Graded::Sum(out)
            }
            CExpr::Table { key, entries, default, span } => self.table(key, entries, default, *span, env)?,
            CExpr::Machine { pitch, dual, body, span } => self.machine(*pitch, *dual, body, *span, env)?,
        })
    }

    fn keyfn(&self, id: &Id, slot: u8, env: &Env) -> R<KeyFn> {
        Ok(match id.name.as_str() {
            "pitch" => KeyFn::Pitch(slot),
            "dur" => KeyFn::Dur(slot),
            "carry" => KeyFn::Carry(slot),
            "prev" => KeyFn::Prev,
            "held" => KeyFn::Held,
            "step" => KeyFn::Step(slot),
            _ => {
                return Err(diag(
                    &env.file,
                    id.span,
                    format!("unknown table key `{}` (pitch, dur, carry, prev, held, step)", id.name),
                ))
            }
        })
    }

    fn table(
        &mut self,
        key: &[(Id, u8)],
        entries: &[(Vec<KeyPat>, Q)],
        default: &Option<Q>,
        span: Span,
        env: &Env,
    ) -> R<Graded> {
        let fns: Vec<KeyFn> = key
            .iter()
            .map(|(id, s)| self.keyfn(id, if *s == 0 { 0 } else { s - 1 }, env))
            .collect::<R<_>>()?;
        if fns.len() > 3 {
            return Err(diag(&env.file, span, "a table key has at most three components"));
        }
        let mut map = BTreeMap::new();
        for (pats, w) in entries {
            if pats.len() != fns.len() {
                return Err(diag(&env.file, span, format!("a key of this table has {} components", fns.len())));
            }
            let mut combos: Vec<Vec<KeyVal>> = vec![vec![]];
            for (p, fnk) in pats.iter().zip(fns.iter()) {
                let vals: Vec<KeyVal> = match (p, fnk) {
                    (KeyPat::Int(k), KeyFn::Step(_)) => vec![KeyVal::I(*k)],
                    (KeyPat::Range(a, b), KeyFn::Step(_)) => {
                        if b < a || b - a > 10_000 {
                            return Err(diag(&env.file, span, "bad step range"));
                        }
                        (*a..=*b).map(KeyVal::I).collect()
                    }
                    (KeyPat::Datum(id), KeyFn::Step(_)) => {
                        return Err(diag(&env.file, id.span, "a step key is an integer or a range"))
                    }
                    (KeyPat::Datum(id), _) => vec![KeyVal::D(self.datum(id, env, None)?)],
                    _ => return Err(diag(&env.file, span, "an integer key needs a `step` component")),
                };
                let mut next = vec![];
                for c in &combos {
                    for v in &vals {
                        let mut d = c.clone();
                        d.push(*v);
                        next.push(d);
                    }
                }
                combos = next;
            }
            for c in combos {
                let mut k = [KeyVal::Pad; 3];
                for (i, v) in c.into_iter().enumerate() {
                    k[i] = v;
                }
                if map.insert(k, w.clone()).is_some() {
                    return Err(diag(&env.file, span, "a table key is listed twice"));
                }
            }
        }
        Ok(Graded::Table(f::Table { key: fns, entries: map, default: default.clone().unwrap_or(Q::ZERO) }))
    }

    fn machine(&mut self, pitch: bool, dual: bool, body: &MachineBody, span: Span, env: &Env) -> R<Graded> {
        let states: Vec<Datum> = if pitch {
            self.alph.all_pitches()
        } else {
            self.alph.positive_durations().to_vec()
        };
        let mut edges: Vec<(Datum, Datum, Q)> = vec![];
        match body {
            MachineBody::Uniform => {
                for &s in &states {
                    for &t in &states {
                        edges.push((s, t, Q::ONE));
                    }
                }
            }
            MachineBody::Edges(v) => {
                for (s, t, w) in v {
                    let want = Some(if pitch { Sort::Pitch } else { Sort::Dur });
                    let sd = self.datum(s, env, want)?;
                    let td = self.datum(t, env, want)?;
                    if !pitch && (!self.alph.len(sd).is_positive() || !self.alph.len(td).is_positive()) {
                        return Err(diag(&env.file, s.span, "duration machines range over durations of positive length"));
                    }
                    if !w.is_positive() {
                        return Err(diag(&env.file, s.span, "machine weights are positive (an absent arrow is weight 0)"));
                    }
                    edges.push((sd, td, w.clone()));
                }
            }
            MachineBody::Indexed { n, edges: ie } => {
                let map: Vec<Datum> = if pitch {
                    let ord = self.alph.ordered_pitches();
                    if *n != ord.len() + 1 {
                        return Err(diag(
                            &env.file,
                            span,
                            format!(
                                "the imported pitch machine has {n} states but the score has {} ordered pitches and a rest",
                                ord.len()
                            ),
                        ));
                    }
                    ord.iter().copied().chain(std::iter::once(self.alph.rest())).collect()
                } else {
                    if *n != states.len() {
                        return Err(diag(
                            &env.file,
                            span,
                            format!("the imported duration machine has {n} states but the score has {} positive durations", states.len()),
                        ));
                    }
                    states.clone()
                };
                for (s, t, w) in ie {
                    edges.push((map[*s], map[*t], w.clone()));
                }
            }
        }
        let key = match (pitch, dual) {
            (true, false) => vec![KeyFn::Pitch(0), KeyFn::Carry(0)],
            (true, true) => vec![KeyFn::Prev, KeyFn::Pitch(0)],
            (false, false) => vec![KeyFn::Prev, KeyFn::Dur(0)],
            (false, true) => vec![KeyFn::Dur(0), KeyFn::Carry(0)],
        };
        let mut map = BTreeMap::new();
        for (s, t, w) in edges {
            if map.insert([KeyVal::D(s), KeyVal::D(t), KeyVal::Pad], w).is_some() {
                return Err(diag(&env.file, span, "a machine lists an arrow twice"));
            }
        }
        Ok(Graded::Table(f::Table { key, entries: map, default: Q::ZERO }))
    }

    fn crisp(&mut self, b: &BExpr, env: &mut Env) -> R<Crisp> {
        Ok(match b {
            BExpr::True => Crisp::True,
            BExpr::False => Crisp::False,
            BExpr::Not(x) => Crisp::Not(Box::new(self.crisp(x, env)?)),
            BExpr::And(v) => Crisp::And(v.iter().map(|x| self.crisp(x, env)).collect::<R<_>>()?),
            BExpr::Or(v) => Crisp::Or(v.iter().map(|x| self.crisp(x, env)).collect::<R<_>>()?),
            BExpr::Leads(c) => Crisp::Leads(Box::new(self.cfg(c, env)?)),
            BExpr::Any { gens, body } | BExpr::All { gens, body } => {
                let combos = self.product(gens, env)?;
                let mut out = vec![];
                for cb in combos {
                    let mark = env.scope.len();
                    for (k, d) in cb {
                        env.scope.push((k, Val::Datum(d)));
                    }
                    let r = self.crisp(body, env);
                    env.scope.truncate(mark);
                    out.push(r?);
                }
                if matches!(b, BExpr::Any { .. }) {
                    Crisp::Or(out)
                } else {
                    Crisp::And(out)
                }
            }
            BExpr::Atom(a) => match a {
                AtomAst::Pitch(s, d) => Crisp::Atom(f::Atom::Pitch(*s, self.datum(d, env, Some(Sort::Pitch))?)),
                AtomAst::Dur(s, d) => Crisp::Atom(f::Atom::Dur(*s, self.datum(d, env, Some(Sort::Dur))?)),
                AtomAst::Carry(s, d) => Crisp::Atom(f::Atom::Carry(*s, self.datum(d, env, None)?)),
                AtomAst::Step(s, k) => Crisp::Atom(f::Atom::Step(*s, *k)),
                AtomAst::At(sp) => Crisp::Atom(f::Atom::At(self.sp(sp, env)?)),
                AtomAst::Passes(s, sp) => Crisp::Atom(f::Atom::Passes(*s, self.sp(sp, env)?)),
                AtomAst::Held(d) => f::held(self.datum(d, env, None)?),
                AtomAst::Prev(d) => f::prev(self.datum(d, env, None)?),
                AtomAst::Back(j, d) => f::back(*j, self.datum(d, env, None)?),
                AtomAst::Last(v) => f::last(&v.iter().map(|x| self.datum(x, env, None)).collect::<R<Vec<_>>>()?),
                AtomAst::Call(v) => f::call(&v.iter().map(|x| self.datum(x, env, None)).collect::<R<Vec<_>>>()?),
            },
        })
    }

    fn sp(&mut self, s: &SpAst, env: &mut Env) -> R<Sp> {
        Ok(match s {
            SpAst::True => Sp::True,
            SpAst::Zero => Sp::Zero,
            SpAst::Not(x) => Sp::Not(Box::new(self.sp(x, env)?)),
            SpAst::And(v) => Sp::And(v.iter().map(|x| self.sp(x, env)).collect::<R<_>>()?),
            SpAst::Or(v) => Sp::Or(v.iter().map(|x| self.sp(x, env)).collect::<R<_>>()?),
            SpAst::Par(a, b) => Sp::Par(Box::new(self.sp(a, env)?), Box::new(self.sp(b, env)?)),
            SpAst::Out { subj, payload, ptimbre, carry } => Sp::Out {
                subj: self.npat(subj, env)?,
                payload: Box::new(self.sp(payload, env)?),
                ptimbre: ptimbre.as_ref().map(|t| self.timbre(t, env)).transpose()?,
                carry: carry.as_ref().map(|d| self.datum(d, env, None)).transpose()?,
            },
            SpAst::In { subj, body } => Sp::In { subj: self.npat(subj, env)?, body: Box::new(self.sp(body, env)?) },
        })
    }

    fn npat(&mut self, n: &NPat, env: &mut Env) -> R<f::NamePat> {
        Ok(f::NamePat {
            quote: match &n.quote {
                Some(q) => Some(Box::new(self.sp(q, env)?)),
                None => None,
            },
            timbre: n.timbre.as_ref().map(|t| self.timbre(t, env)).transpose()?,
            datum: n.datum.as_ref().map(|d| self.datum(d, env, None)).transpose()?,
        })
    }

    fn cfg(&mut self, c: &CfgAst, env: &mut Env) -> R<f::Cfg> {
        Ok(match c {
            CfgAst::True => f::Cfg::True,
            CfgAst::False => f::Cfg::False,
            CfgAst::Live => f::Cfg::Live,
            CfgAst::Not(x) => f::Cfg::Not(Box::new(self.cfg(x, env)?)),
            CfgAst::And(v) => f::Cfg::And(v.iter().map(|x| self.cfg(x, env)).collect::<R<_>>()?),
            CfgAst::Or(v) => f::Cfg::Or(v.iter().map(|x| self.cfg(x, env)).collect::<R<_>>()?),
            CfgAst::Dia { guard, body } => f::Cfg::Dia {
                guard: match guard {
                    Some(g) => Some(Box::new(self.crisp(g, env)?)),
                    None => None,
                },
                body: Box::new(self.cfg(body, env)?),
            },
            CfgAst::Nu(x, body) => {
                let n = self.nu_counter;
                self.nu_counter += 1;
                env.nu.push((x.name.clone(), n));
                let b = self.cfg(body, env);
                env.nu.pop();
                f::Cfg::Nu(n, Box::new(b?))
            }
            CfgAst::Var(x) => match env.nu.iter().rev().find(|(k, _)| *k == x.name) {
                Some((_, n)) => f::Cfg::Var(*n),
                None => return Err(diag(&env.file, x.span, format!("unbound fixed-point variable `{}`", x.name))),
            },
        })
    }
}

fn kind(v: &Val) -> &'static str {
    match v {
        Val::Proc(_) => "process",
        Val::Name(_) => "name",
        Val::Timbre(_) => "timbre",
        Val::Datum(_) => "datum",
        Val::Clause(_) => "clause",
        Val::Str(_) => "string",
        Val::Cont(_) => "continuation",
    }
}
