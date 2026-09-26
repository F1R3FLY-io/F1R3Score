//! The recursive-descent parser for `.score` files.

use crate::ast::*;
use crate::lexer::{lex, Diag, Span, Tok};
use score_core::Q;
use std::collections::{HashMap, HashSet};

/// Supplies the text of imported files (`import "x.score"`) and imported
/// machines (`machine pitch import "x.machine"`). The syntax crate performs
/// no I/O itself; `std` is built in.
pub trait Loader {
    fn load(&mut self, path: &str) -> Result<String, String>;
}

/// A loader that refuses every file (for embedded use).
pub struct NoFiles;
impl Loader for NoFiles {
    fn load(&mut self, path: &str) -> Result<String, String> {
        Err(format!("no file loader available for `{path}`"))
    }
}

pub const STD_SCORE: &str = include_str!("../std.score");

struct Parser<'l> {
    toks: Vec<(Tok, Span)>,
    pos: usize,
    file: String,
    sigs: HashMap<String, Vec<Sort>>,
    /// definitions that return a name
    name_defs: HashSet<String>,
    params: HashMap<String, Sort>,
    factors: Vec<String>,
    loader: &'l mut dyn Loader,
    imported: Vec<String>,
    out: File,
    /// > 0 while parsing definition arguments, where `;` separates arguments
    /// rather than sequencing output
    no_seq: u32,
}

type R<T> = Result<T, Diag>;

pub fn parse(file: &str, src: &str, loader: &mut dyn Loader) -> R<File> {
    let mut p = Parser {
        toks: lex(file, src)?,
        pos: 0,
        file: file.into(),
        sigs: HashMap::new(),
        name_defs: HashSet::new(),
        params: HashMap::new(),
        factors: vec![],
        loader,
        imported: vec![],
        out: File { file: file.into(), ..Default::default() },
        no_seq: 0,
    };
    p.file_items()?;
    Ok(p.out)
}

impl<'l> Parser<'l> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }
    fn span(&self) -> Span {
        self.toks[self.pos].1
    }
    fn bump(&mut self) -> (Tok, Span) {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(Diag { file: self.file.clone(), span: self.span(), msg: msg.into() })
    }
    fn is_p(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(q) if *q == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(q) if q == k)
    }
    fn eat_p(&mut self, p: &str) -> bool {
        if self.is_p(p) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_p(&mut self, p: &str) -> R<Span> {
        if self.is_p(p) {
            Ok(self.bump().1)
        } else {
            self.err(format!("expected `{p}`, found {}", self.peek()))
        }
    }
    fn expect_kw(&mut self, k: &str) -> R<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            self.err(format!("expected `{k}`, found {}", self.peek()))
        }
    }
    fn ident(&mut self) -> R<Id> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                let sp = self.bump().1;
                Ok(Id { name: s, span: sp })
            }
            t => self.err(format!("expected an identifier, found {t}")),
        }
    }
    fn int(&mut self) -> R<u64> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(n)
            }
            t => self.err(format!("expected an integer, found {t}")),
        }
    }
    fn signed(&mut self) -> R<i64> {
        let neg = self.eat_p("-");
        let n = self.int()? as i64;
        Ok(if neg { -n } else { n })
    }
    fn string(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Str(s) => {
                self.bump();
                Ok(s)
            }
            t => self.err(format!("expected a string, found {t}")),
        }
    }
    /// `3`, `3/8`, `0.3`
    fn number(&mut self) -> R<Q> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                if self.eat_p("/") {
                    let d = self.int()?;
                    if d == 0 {
                        return self.err("zero denominator");
                    }
                    Ok(Q::parse(&format!("{n}/{d}")).unwrap())
                } else {
                    Ok(Q::int(n))
                }
            }
            Tok::Dec(s) => {
                self.bump();
                Ok(Q::parse(&s).unwrap())
            }
            t => self.err(format!("expected a number, found {t}")),
        }
    }
    fn sep_list<T>(&mut self, close: &str, mut f: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let mut v = vec![];
        if self.eat_p(close) {
            return Ok(v);
        }
        loop {
            v.push(f(self)?);
            if self.eat_p(",") {
                if self.eat_p(close) {
                    return Ok(v);
                }
                continue;
            }
            self.expect_p(close)?;
            return Ok(v);
        }
    }

    // --------------------------------------------------------------- items

    fn file_items(&mut self) -> R<()> {
        if self.eat_kw("score") {
            let n = self.ident()?;
            self.out.name = Some(n.name);
        }
        loop {
            match self.peek().clone() {
                Tok::Eof => return Ok(()),
                Tok::Ident(k) => match k.as_str() {
                    "import" => self.import()?,
                    "pitches" => self.pitches()?,
                    "durations" => self.durations()?,
                    "timbres" => self.timbres()?,
                    "def" => self.def()?,
                    "factor" => self.factor()?,
                    "pitchset" => self.pitchset_decl()?,
                    "play" => {
                        let sp = self.bump().1;
                        if self.out.play.is_some() {
                            return Err(Diag {
                                file: self.file.clone(),
                                span: sp,
                                msg: "a score has exactly one `play` item".into(),
                            });
                        }
                        let e = self.proc()?;
                        self.out.play = Some((e, sp));
                    }
                    _ => return self.err(format!("expected a declaration, definition or `play`, found `{k}`")),
                },
                t => return self.err(format!("expected a declaration, found {t}")),
            }
        }
    }

    fn import(&mut self) -> R<()> {
        self.bump();
        let (name, src) = match self.peek().clone() {
            Tok::Ident(s) if s == "std" => {
                self.bump();
                ("std.score".to_string(), STD_SCORE.to_string())
            }
            Tok::Str(s) => {
                self.bump();
                let src = self.loader.load(&s).or_else(|e| self.err(format!("cannot import `{s}`: {e}")))?;
                (s, src)
            }
            t => return self.err(format!("expected `std` or a file name after `import`, found {t}")),
        };
        if self.imported.contains(&name) {
            return Ok(());
        }
        self.imported.push(name.clone());
        let toks = lex(&name, &src)?;
        let saved = (std::mem::replace(&mut self.toks, toks), self.pos, std::mem::replace(&mut self.file, name));
        self.pos = 0;
        let r = self.library_items();
        self.toks = saved.0;
        self.pos = saved.1;
        self.file = saved.2;
        r
    }

    /// An imported file contributes definitions and factors only.
    fn library_items(&mut self) -> R<()> {
        loop {
            match self.peek().clone() {
                Tok::Eof => return Ok(()),
                Tok::Ident(k) if k == "def" => self.def()?,
                Tok::Ident(k) if k == "factor" => self.factor()?,
                Tok::Ident(k) if k == "pitchset" => self.pitchset_decl()?,
                Tok::Ident(k) if k == "import" => self.import()?,
                t => {
                    return self.err(format!(
                        "an imported file may contain only `def`, `factor`, `pitchset` and `import`, found {t}"
                    ))
                }
            }
        }
    }

    fn pitches(&mut self) -> R<()> {
        let sp = self.bump().1;
        self.expect_p("{")?;
        let v = self.sep_list("}", |p| {
            if p.is_kw("piano88") {
                let sp = p.bump().1;
                return Ok(PitchEntry::Piano88(sp));
            }
            if p.eat_kw("scale") {
                let mut kind = p.ident()?.name;
                while p.eat_p("-") {
                    kind.push('-');
                    kind.push_str(&p.ident()?.name);
                }
                let lo = p.ident()?;
                p.expect_p("..")?;
                let hi = p.ident()?;
                Ok(PitchEntry::Scale { kind, lo, hi })
            } else {
                let id = p.ident()?;
                let midi = if p.eat_p("=") {
                    let n = p.int()?;
                    if n > 127 {
                        return p.err("MIDI numbers are 0..127");
                    }
                    Some(n as u8)
                } else {
                    None
                };
                Ok(PitchEntry::Named(id, midi))
            }
        })?;
        if self.out.pitches.is_some() {
            return Err(Diag { file: self.file.clone(), span: sp, msg: "`pitches` declared twice".into() });
        }
        self.out.pitches = Some((v, sp));
        Ok(())
    }

    fn durations(&mut self) -> R<()> {
        let sp = self.bump().1;
        self.expect_p("{")?;
        let v = self.sep_list("}", |p| {
            let id = p.ident()?;
            p.expect_p("=")?;
            let q = p.number()?;
            Ok((id, q))
        })?;
        if self.out.durations.is_some() {
            return Err(Diag { file: self.file.clone(), span: sp, msg: "`durations` declared twice".into() });
        }
        self.out.durations = Some((v, sp));
        Ok(())
    }

    fn timbres(&mut self) -> R<()> {
        let sp = self.bump().1;
        self.expect_p("{")?;
        let v = self.sep_list("}", |p| {
            let name = p.ident()?;
            let mut program = None;
            let mut channel = None;
            if p.eat_p("=") {
                p.expect_kw("gm")?;
                p.expect_p("(")?;
                let n = p.int()?;
                if n > 127 {
                    return p.err("General MIDI programs are 0..127");
                }
                program = Some(n as u8);
                p.expect_p(")")?;
            }
            if p.eat_kw("on") {
                let c = p.int()?;
                if !(1..=16).contains(&c) {
                    return p.err("MIDI channels are 1..16");
                }
                channel = Some(c as u8);
            }
            Ok(TimbreEntry { name, program, channel })
        })?;
        if self.out.timbres.is_some() {
            return Err(Diag { file: self.file.clone(), span: sp, msg: "`timbres` declared twice".into() });
        }
        self.out.timbres = Some((v, sp));
        Ok(())
    }

    /// A parameter sort: `proc`, ..., `pitchset`, `[S]`, `(S1, S2, ...)`.
    fn sort(&mut self) -> R<Sort> {
        if self.eat_p("[") {
            let s = self.sort()?;
            self.expect_p("]")?;
            return Ok(Sort::List(Box::new(s)));
        }
        if self.eat_p("(") {
            let v = self.sep_list(")", |p| p.sort())?;
            return Ok(Sort::Tuple(v));
        }
        let s = self.ident()?;
        Sort::parse(&s.name).ok_or_else(|| Diag {
            file: self.file.clone(),
            span: s.span,
            msg: format!(
                "unknown sort `{}` (proc, name, timbre, pitch, dur, clause, cont, string, pitchset, [S], (S, ...))",
                s.name
            ),
        })
    }

    fn def(&mut self) -> R<()> {
        self.bump();
        let name = self.ident()?;
        if self.sigs.contains_key(&name.name) {
            return Err(Diag {
                file: self.file.clone(),
                span: name.span,
                msg: format!("`{}` is defined twice", name.name),
            });
        }
        let mut params: Vec<(Id, Sort)> = vec![];
        // `def P1 = base "P1"`: a definition without parameters
        if self.eat_p("(") && !self.eat_p(")") {
            loop {
                let pid = self.ident()?;
                self.expect_p(":")?;
                let sort = self.sort()?;
                if params.iter().any(|(q, _)| q.name == pid.name) {
                    return Err(Diag {
                        file: self.file.clone(),
                        span: pid.span,
                        msg: format!("parameter `{}` repeated", pid.name),
                    });
                }
                params.push((pid, sort));
                if self.eat_p(",") || self.eat_p(";") {
                    continue;
                }
                self.expect_p(")")?;
                break;
            }
        }
        let returns_name = if self.eat_p(":") {
            let s = self.ident()?;
            if s.name != "name" {
                return Err(Diag {
                    file: self.file.clone(),
                    span: s.span,
                    msg: "a definition returns a process, or a name (`: name`)".into(),
                });
            }
            true
        } else {
            false
        };
        self.expect_p("=")?;
        // registering the signature first lets a recursive definition parse,
        // so that elaboration can reject it with the right diagnostic
        self.sigs.insert(name.name.clone(), params.iter().map(|x| x.1.clone()).collect());
        if returns_name {
            self.name_defs.insert(name.name.clone());
        }
        self.params = params.iter().map(|(i, s)| (i.name.clone(), s.clone())).collect();
        let body = if returns_name { self.name_expr().map(DefBody::Name) } else { self.proc().map(DefBody::Proc) };
        self.params.clear();
        let body = body?;
        self.out.defs.push(Def { name, params, body, file: self.file.clone() });
        Ok(())
    }

    fn pitchset_decl(&mut self) -> R<()> {
        self.bump();
        let name = self.ident()?;
        self.expect_p("=")?;
        self.expect_p("{")?;
        let v = self.sep_list("}", |p| p.ident())?;
        self.out.pitchsets.push((name, v));
        Ok(())
    }

    fn factor(&mut self) -> R<()> {
        self.bump();
        let name = self.ident()?;
        self.expect_p("=")?;
        let c = self.clause()?;
        self.factors.push(name.name.clone());
        self.out.factors.push((name, c));
        Ok(())
    }

    // --------------------------------------------------------------- procs

    pub fn proc(&mut self) -> R<PExpr> {
        let mut v = vec![self.seq()?];
        while self.eat_p("|") {
            v.push(self.seq()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { PExpr::Par(v) })
    }

    /// `step ; rest`, right-associative and binding more tightly than `|`
    /// (Def. 9.3). The step is one message, or a parenthesised parallel group
    /// of messages.
    fn seq(&mut self) -> R<PExpr> {
        let sp = self.span();
        let a = self.proc_atom()?;
        if self.no_seq > 0 || !self.is_p(";") {
            return Ok(a);
        }
        let step = match &a {
            PExpr::Send { .. } => vec![a],
            PExpr::Par(v) if v.iter().all(|x| matches!(x, PExpr::Send { .. })) => v.clone(),
            _ => {
                return self.err(
                    "the left of `;` must be a message, or a parenthesised parallel group of messages \
                     (synchronous output, Definition 9.3)",
                )
            }
        };
        self.bump();
        let rest = self.seq()?;
        Ok(PExpr::Seq { step, rest: Box::new(rest), span: sp })
    }

    fn proc_atom(&mut self) -> R<PExpr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Int(0) => {
                self.bump();
                Ok(PExpr::Zero)
            }
            Tok::P("(") => {
                self.bump();
                let saved = std::mem::replace(&mut self.no_seq, 0);
                let e = self.proc();
                self.no_seq = saved;
                let e = e?;
                self.expect_p(")")?;
                Ok(e)
            }
            Tok::P("*") => {
                self.bump();
                let n = self.name_expr()?;
                Ok(PExpr::Drop(n))
            }
            Tok::P("<") | Tok::P("@") => {
                let n = self.name_expr()?;
                self.send_rest(n, sp)
            }
            Tok::Ident(k) => match k.as_str() {
                "for" => self.for_expr(),
                "par" => {
                    self.bump();
                    let gens = self.gens()?;
                    self.expect_p("{")?;
                    let body = self.proc()?;
                    self.expect_p("}")?;
                    Ok(PExpr::ParComp { gens, body: Box::new(body) })
                }
                "at" if !self.params.contains_key("at") => {
                    self.bump();
                    let t = self.number()?;
                    self.expect_p("{")?;
                    let body = self.proc()?;
                    self.expect_p("}")?;
                    Ok(PExpr::At { t, body: Box::new(body), span: sp })
                }
                "base" if !self.params.contains_key("base") => {
                    self.bump();
                    Ok(PExpr::Base(self.str_expr()?))
                }
                "line" if !self.sigs.contains_key("line") => self.line(),
                "keyloc" | "keycode" if !self.params.contains_key(&k) && !self.sigs.contains_key(&k) => {
                    self.bump();
                    self.expect_p("(")?;
                    let p = self.proc()?;
                    self.expect_p(",")?;
                    let key = self.deco()?;
                    let e = if k == "keycode" {
                        self.expect_p(",")?;
                        let tau = self.deco()?;
                        PExpr::KeyCode { p: Box::new(p), k: key, tau, span: sp }
                    } else {
                        PExpr::KeyLoc { p: Box::new(p), k: key, span: sp }
                    };
                    self.expect_p(")")?;
                    Ok(e)
                }
                "ackloc" if !self.params.contains_key(&k) && !self.sigs.contains_key(&k) => {
                    self.bump();
                    self.expect_p("(")?;
                    let n = self.int()?;
                    self.expect_p(")")?;
                    Ok(PExpr::AckLoc(n))
                }
                _ => {
                    let id = self.ident()?;
                    if self.is_p("!") {
                        return self.send_rest(NExpr::Ident(id), sp);
                    }
                    if self.is_p("(") && self.name_defs.contains(&id.name) {
                        let args = self.app_args(&id)?;
                        return self.send_rest(NExpr::App { def: id, args }, sp);
                    }
                    if !self.is_p("(") && !self.params.contains_key(&id.name) && self.sigs.get(&id.name).map_or(false, |s| s.is_empty()) {
                        // a definition without parameters: `P1`
                        return Ok(PExpr::App { def: id, args: vec![], label: None });
                    }
                    if self.is_p("(") {
                        if self.params.get(&id.name) == Some(&Sort::Cont) {
                            self.bump();
                            let n = self.name_expr()?;
                            self.expect_p(")")?;
                            return Ok(PExpr::ContApp { param: id, name: n });
                        }
                        let args = self.app_args(&id)?;
                        let label = if self.eat_p("#") { Some(self.ident()?) } else { None };
                        return Ok(PExpr::App { def: id, args, label });
                    }
                    Ok(PExpr::Ref(id))
                }
            },
            t => self.err(format!("expected a process, found {t}")),
        }
    }

    /// A decoration component: a name or `_`.
    fn deco(&mut self) -> R<DecoId> {
        if self.is_p("_") {
            return Ok(DecoId::Wild(self.bump().1));
        }
        Ok(DecoId::Id(self.ident()?))
    }

    /// `!(Q, t, d)` or `!(Q)`, which passes the general name `@Q`.
    fn send_rest(&mut self, subj: NExpr, sp: Span) -> R<PExpr> {
        self.expect_p("!")?;
        self.expect_p("(")?;
        let saved = std::mem::replace(&mut self.no_seq, 0);
        let payload = self.proc();
        self.no_seq = saved;
        let payload = payload?;
        let deco = if self.eat_p(",") {
            let timbre = self.deco()?;
            self.expect_p(",")?;
            let datum = self.deco()?;
            Some((timbre, datum))
        } else {
            None
        };
        self.expect_p(")")?;
        Ok(PExpr::Send { subj, payload: Box::new(payload), deco, span: sp })
    }

    fn for_expr(&mut self) -> R<PExpr> {
        let sp = self.bump().1;
        let label = if self.eat_p("#") { Some(self.ident()?) } else { None };
        self.expect_p("(")?;
        let mut binds = vec![];
        loop {
            let b = if self.eat_p("_") { None } else { Some(self.ident()?) };
            self.expect_p("<-")?;
            let n = self.name_expr()?;
            binds.push((b, n));
            if self.eat_p("&") {
                continue;
            }
            break;
        }
        let clause = if self.eat_kw("where") { Some(self.clause()?) } else { None };
        self.expect_p(")")?;
        self.expect_p("{")?;
        let body = self.proc()?;
        self.expect_p("}")?;
        Ok(PExpr::For { label, binds, clause, body: Box::new(body), span: sp })
    }

    fn line(&mut self) -> R<PExpr> {
        self.bump();
        self.expect_p("(")?;
        let loc = self.proc()?;
        self.expect_p(",")?;
        let timbre = self.ident()?;
        self.expect_p(")")?;
        self.expect_p("[")?;
        let notes = self.sep_list("]", |p| {
            let a = p.ident()?;
            let b = p.ident()?;
            Ok((a, b))
        })?;
        let then = if self.eat_kw("then") { Some(Box::new(self.proc_atom()?)) } else { None };
        Ok(PExpr::Line { loc: Box::new(loc), timbre, notes, then })
    }

    fn name_expr(&mut self) -> R<NExpr> {
        if self.eat_p("<") {
            self.expect_p("@")?;
            let saved = std::mem::replace(&mut self.no_seq, 0);
            let p = self.proc();
            self.no_seq = saved;
            let p = p?;
            self.expect_p(",")?;
            let timbre = self.deco()?;
            self.expect_p(",")?;
            let datum = self.deco()?;
            self.expect_p(">")?;
            return Ok(NExpr::Quote { proc: Box::new(p), timbre, datum });
        }
        if self.is_p("@") {
            // the general name @P = <@P, _, _>
            let sp = self.bump().1;
            let p = self.proc_atom()?;
            return Ok(NExpr::Quote { proc: Box::new(p), timbre: DecoId::Wild(sp), datum: DecoId::Wild(sp) });
        }
        let id = self.ident()?;
        if self.is_p("(") && self.name_defs.contains(&id.name) {
            let args = self.app_args(&id)?;
            return Ok(NExpr::App { def: id, args });
        }
        Ok(NExpr::Ident(id))
    }

    fn str_expr(&mut self) -> R<SExpr> {
        let mut e = match self.peek().clone() {
            Tok::Str(s) => {
                self.bump();
                SExpr::Lit(s)
            }
            Tok::Ident(_) => SExpr::Ref(self.ident()?),
            t => return self.err(format!("expected a string, found {t}")),
        };
        while self.eat_p("++") {
            let r = match self.peek().clone() {
                Tok::Str(s) => {
                    self.bump();
                    SExpr::Lit(s)
                }
                Tok::Ident(_) => SExpr::Ref(self.ident()?),
                t => return self.err(format!("expected a string, found {t}")),
            };
            e = SExpr::Concat(Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn app_args(&mut self, def: &Id) -> R<Vec<Arg>> {
        let sig = match self.sigs.get(&def.name) {
            Some(s) => s.clone(),
            None => {
                return Err(Diag {
                    file: self.file.clone(),
                    span: def.span,
                    msg: format!("`{}` is not a definition (definitions must precede their use)", def.name),
                })
            }
        };
        self.expect_p("(")?;
        self.no_seq += 1;
        let r = self.app_args_inner(def, &sig);
        self.no_seq -= 1;
        r
    }

    fn app_args_inner(&mut self, def: &Id, sig: &[Sort]) -> R<Vec<Arg>> {
        let mut args = vec![];
        for (i, s) in sig.iter().enumerate() {
            if i > 0 && !self.eat_p(",") && !self.eat_p(";") {
                return self.err(format!("`{}` takes {} arguments; expected `,`", def.name, sig.len()));
            }
            args.push(self.arg(s)?);
        }
        if !self.is_p(")") {
            return self.err(format!("`{}` takes {} arguments", def.name, sig.len()));
        }
        self.bump();
        Ok(args)
    }

    fn pset(&mut self) -> R<PSet> {
        if self.eat_p("{") {
            return Ok(PSet::Lit(self.sep_list("}", |p| p.ident())?));
        }
        if self.eat_kw("pitches") {
            if self.eat_p("-") {
                self.expect_kw("r")?;
                return Ok(PSet::PitchesR);
            }
            return Ok(PSet::Pitches);
        }
        Ok(PSet::Ref(self.ident()?))
    }

    fn arg(&mut self, s: &Sort) -> R<Arg> {
        if self.is_p("_") {
            let sp = self.bump().1;
            return Ok(match s {
                // the continuation's hole
                Sort::Name => Arg::Hole(sp),
                // a wildcard decoration
                Sort::Timbre => Arg::Timbre(DecoId::Wild(sp)),
                Sort::Pitch | Sort::Dur => Arg::Datum(DecoId::Wild(sp)),
                _ => return Err(Diag { file: self.file.clone(), span: sp, msg: format!("`_` is not a {}", s.show()) }),
            });
        }
        Ok(match s {
            Sort::Proc => Arg::Proc(self.proc()?),
            Sort::Name => Arg::Name(self.name_expr()?),
            Sort::Timbre => Arg::Timbre(DecoId::Id(self.ident()?)),
            Sort::Pitch | Sort::Dur => Arg::Datum(DecoId::Id(self.ident()?)),
            Sort::Pitchset => Arg::Pitchset(self.pset()?),
            Sort::List(inner) => {
                if matches!(self.peek(), Tok::Ident(_)) {
                    // a list parameter passed on
                    let id = self.ident()?;
                    return Ok(Arg::Name(NExpr::Ident(id)));
                }
                let sp = self.expect_p("[")?;
                let inner = (**inner).clone();
                let v = self.sep_list("]", |p| p.arg(&inner))?;
                Arg::List(v, sp)
            }
            Sort::Tuple(parts) => {
                let sp = self.expect_p("(")?;
                let mut v = vec![];
                for (i, q) in parts.iter().enumerate() {
                    if i > 0 {
                        self.expect_p(",")?;
                    }
                    v.push(self.arg(q)?);
                }
                self.expect_p(")")?;
                Arg::Tuple(v, sp)
            }
            Sort::Clause => Arg::Clause(self.clause()?),
            Sort::Str => Arg::Str(self.str_expr()?),
            Sort::Cont => {
                let def = self.ident()?;
                if !self.is_p("(") {
                    // a cont parameter passed on
                    return Ok(Arg::Cont(ContExpr { def, args: vec![] }));
                }
                let args = self.app_args(&def)?;
                Arg::Cont(ContExpr { def, args })
            }
        })
    }

    fn gens(&mut self) -> R<Vec<Gen>> {
        let mut v = vec![];
        loop {
            let (vars, tuple) = if self.eat_p("(") {
                (self.sep_list(")", |p| p.ident())?, true)
            } else {
                (vec![self.ident()?], false)
            };
            self.expect_kw("in")?;
            let set = if self.eat_kw("pitches") {
                if self.eat_p("-") {
                    self.expect_kw("r")?;
                    GenSet::PitchesR
                } else {
                    GenSet::Pitches
                }
            } else if self.eat_kw("durations") {
                if self.eat_p("+") {
                    GenSet::DurationsPlus
                } else {
                    GenSet::Durations
                }
            } else if self.eat_p("[") {
                GenSet::List(self.sep_list("]", |p| p.ident())?)
            } else if self.eat_p("{") {
                GenSet::List(self.sep_list("}", |p| p.ident())?)
            } else if matches!(self.peek(), Tok::Ident(_)) {
                GenSet::Ref(self.ident()?)
            } else {
                return self.err(
                    "expected `pitches`, `pitches-r`, `durations`, `durations+`, a list, or a pitch set or list parameter",
                );
            };
            v.push(Gen { vars, tuple, set });
            if self.eat_p(",") {
                continue;
            }
            return Ok(v);
        }
    }

    // ------------------------------------------------------------- clauses

    pub fn clause(&mut self) -> R<CExpr> {
        let mut v = vec![self.cprod()?];
        while self.eat_p("+") {
            v.push(self.cprod()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { CExpr::Add(v) })
    }
    fn cprod(&mut self) -> R<CExpr> {
        let mut v = vec![self.cunit()?];
        while self.eat_p("*") {
            v.push(self.cunit()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { CExpr::Mul(v) })
    }
    fn cunit(&mut self) -> R<CExpr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Int(_) | Tok::Dec(_) => Ok(CExpr::Num(self.number()?)),
            Tok::P("[") => {
                self.bump();
                let b = self.crisp()?;
                self.expect_p("]")?;
                Ok(CExpr::Crisp(b))
            }
            Tok::P("(") => {
                self.bump();
                let c = self.clause()?;
                self.expect_p(")")?;
                Ok(c)
            }
            Tok::P("<>") | Tok::P("<") => self.err(
                "graded behavioural modalities are outside the checkable fragment of this player; \
                 write the behavioural condition crisply under `[leads(...)]`",
            ),
            Tok::Ident(k) => match k.as_str() {
                "mu" | "nu" => self.err(
                    "graded fixed points are outside the checkable fragment of this player; \
                     use a crisp `nu` under `leads(...)`",
                ),
                "table" => self.table(sp),
                "machine" => self.machine(sp),
                "sum" => {
                    self.bump();
                    let gens = self.gens()?;
                    self.expect_p("{")?;
                    let body = self.clause()?;
                    self.expect_p("}")?;
                    Ok(CExpr::SumComp { gens, body: Box::new(body) })
                }
                _ => {
                    let id = self.ident()?;
                    Ok(CExpr::Ref(id))
                }
            },
            t => self.err(format!("expected a clause, found {t}")),
        }
    }

    fn keyfn(&mut self) -> R<(Id, u8)> {
        let id = self.ident()?;
        let slot = if self.eat_p("#") { self.int()? as u8 } else { 0 };
        Ok((id, slot))
    }

    fn table(&mut self, span: Span) -> R<CExpr> {
        self.bump();
        let key = if self.eat_p("(") { self.sep_list(")", |p| p.keyfn())? } else { vec![self.keyfn()?] };
        self.expect_p("{")?;
        let mut entries = vec![];
        let mut default = None;
        let n = key.len();
        let items = self.sep_list("}", |p| {
            if p.eat_p("_") {
                p.expect_p(":")?;
                return Ok((None, p.number()?));
            }
            let pats = if n > 1 {
                p.expect_p("(")?;
                p.sep_list(")", |q| q.keypat())?
            } else {
                vec![p.keypat()?]
            };
            p.expect_p(":")?;
            Ok((Some(pats), p.number()?))
        })?;
        for (k, w) in items {
            match k {
                Some(k) => entries.push((k, w)),
                None => default = Some(w),
            }
        }
        Ok(CExpr::Table { key, entries, default, span })
    }

    fn keypat(&mut self) -> R<KeyPat> {
        match self.peek().clone() {
            Tok::Int(_) | Tok::P("-") => {
                let a = self.signed()?;
                if self.eat_p("..") {
                    let b = self.signed()?;
                    Ok(KeyPat::Range(a, b))
                } else {
                    Ok(KeyPat::Int(a))
                }
            }
            _ => Ok(KeyPat::Datum(self.ident()?)),
        }
    }

    fn machine(&mut self, span: Span) -> R<CExpr> {
        self.bump();
        let pitch = if self.eat_kw("pitch") {
            true
        } else if self.eat_kw("dur") {
            false
        } else {
            return self.err("expected `pitch` or `dur` after `machine`");
        };
        let dual = self.eat_kw("dual");
        let body = if self.eat_kw("uniform") {
            MachineBody::Uniform
        } else if self.eat_kw("import") {
            let f = self.string()?;
            let src = self.loader.load(&f).or_else(|e| self.err(format!("cannot import machine `{f}`: {e}")))?;
            parse_machine_file(&f, &src)?
        } else {
            self.expect_p("{")?;
            MachineBody::Edges(self.sep_list("}", |p| {
                let s = p.ident()?;
                p.expect_p("->")?;
                let t = p.ident()?;
                p.expect_p("@")?;
                let w = p.number()?;
                Ok((s, t, w))
            })?)
        };
        Ok(CExpr::Machine { pitch, dual, body, span })
    }

    // --------------------------------------------------------------- crisp

    fn crisp(&mut self) -> R<BExpr> {
        let mut v = vec![self.crisp_and()?];
        while self.eat_p("||") {
            v.push(self.crisp_and()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { BExpr::Or(v) })
    }
    fn crisp_and(&mut self) -> R<BExpr> {
        let mut v = vec![self.crisp_atom()?];
        while self.eat_p("&&") {
            v.push(self.crisp_atom()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { BExpr::And(v) })
    }
    fn slot(&mut self) -> R<u8> {
        if self.eat_p("#") {
            let n = self.int()?;
            if n == 0 || n > 64 {
                return self.err("chord slots are numbered from 1");
            }
            Ok((n - 1) as u8)
        } else {
            Ok(0)
        }
    }
    fn crisp_atom(&mut self) -> R<BExpr> {
        match self.peek().clone() {
            Tok::P("!") => {
                self.bump();
                Ok(BExpr::Not(Box::new(self.crisp_atom()?)))
            }
            Tok::P("(") => {
                self.bump();
                let b = self.crisp()?;
                self.expect_p(")")?;
                Ok(b)
            }
            Tok::P("<>") | Tok::P("<") => {
                self.err("a behavioural modality is a formula of the configuration: write it under `leads(...)`")
            }
            Tok::Ident(k) => {
                let kw = k.clone();
                self.bump();
                let a = match kw.as_str() {
                    "true" => return Ok(BExpr::True),
                    "false" => return Ok(BExpr::False),
                    "nu" => {
                        return self.err(
                            "a fixed point is a formula of the configuration: write it under `leads(...)`",
                        )
                    }
                    "leads" => {
                        self.expect_p("(")?;
                        let c = self.cfg()?;
                        self.expect_p(")")?;
                        return Ok(BExpr::Leads(c));
                    }
                    "any" | "all" => {
                        let gens = self.gens()?;
                        self.expect_p("{")?;
                        let body = Box::new(self.crisp()?);
                        self.expect_p("}")?;
                        return Ok(if kw == "any" { BExpr::Any { gens, body } } else { BExpr::All { gens, body } });
                    }
                    "pitch" | "dur" | "carry" => {
                        let s = self.slot()?;
                        self.expect_p("(")?;
                        let d = self.deco()?;
                        self.expect_p(")")?;
                        match (kw.as_str(), d) {
                            ("carry", d) => AtomAst::Carry(s, d),
                            (_, DecoId::Wild(sp)) => {
                                return Err(Diag {
                                    file: self.file.clone(),
                                    span: sp,
                                    msg: format!("the note's {kw} is always concrete; `{kw}(_)` never holds"),
                                })
                            }
                            ("pitch", DecoId::Id(d)) => AtomAst::Pitch(s, d),
                            (_, DecoId::Id(d)) => AtomAst::Dur(s, d),
                        }
                    }
                    "step" => {
                        let s = self.slot()?;
                        self.expect_p("(")?;
                        let k = self.signed()?;
                        self.expect_p(")")?;
                        AtomAst::Step(s, k)
                    }
                    "at" => {
                        self.expect_p("(")?;
                        let f = self.sp()?;
                        self.expect_p(")")?;
                        AtomAst::At(f)
                    }
                    "passes" => {
                        let s = self.slot()?;
                        self.expect_p("(")?;
                        let f = self.sp()?;
                        self.expect_p(")")?;
                        AtomAst::Passes(s, f)
                    }
                    "held" | "prev" => {
                        self.expect_p("(")?;
                        let d = self.ident()?;
                        self.expect_p(")")?;
                        if kw == "held" {
                            AtomAst::Held(d)
                        } else {
                            AtomAst::Prev(d)
                        }
                    }
                    "back" => {
                        self.expect_p("(")?;
                        let j = self.int()? as u32;
                        self.expect_p(",")?;
                        let d = self.ident()?;
                        self.expect_p(")")?;
                        AtomAst::Back(j, d)
                    }
                    "last" | "call" => {
                        self.expect_p("[")?;
                        let mut v = vec![];
                        while !self.is_p("]") {
                            v.push(self.ident()?);
                            self.eat_p(",");
                        }
                        self.bump();
                        if kw == "last" {
                            AtomAst::Last(v)
                        } else {
                            AtomAst::Call(v)
                        }
                    }
                    _ => return self.err(format!("unknown atom `{kw}`")),
                };
                Ok(BExpr::Atom(a))
            }
            t => self.err(format!("expected a crisp formula, found {t}")),
        }
    }

    // -------------------------------------------------------------- spatial

    fn sp(&mut self) -> R<SpAst> {
        let mut v = vec![self.sp_and()?];
        while self.eat_p("||") {
            v.push(self.sp_and()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { SpAst::Or(v) })
    }
    fn sp_and(&mut self) -> R<SpAst> {
        let mut v = vec![self.sp_par()?];
        while self.eat_p("&&") {
            v.push(self.sp_par()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { SpAst::And(v) })
    }
    fn sp_par(&mut self) -> R<SpAst> {
        let mut e = self.sp_atom()?;
        while self.eat_p("|") {
            let r = self.sp_atom()?;
            e = SpAst::Par(Box::new(e), Box::new(r));
        }
        Ok(e)
    }
    fn sp_atom(&mut self) -> R<SpAst> {
        match self.peek().clone() {
            Tok::P("!") => {
                self.bump();
                Ok(SpAst::Not(Box::new(self.sp_atom()?)))
            }
            Tok::P("(") => {
                self.bump();
                let s = self.sp()?;
                self.expect_p(")")?;
                Ok(s)
            }
            Tok::P("@") => {
                self.bump();
                self.sp_atom()
            }
            Tok::Int(0) => {
                self.bump();
                Ok(SpAst::Zero)
            }
            Tok::Ident(k) if k == "true" => {
                self.bump();
                Ok(SpAst::True)
            }
            Tok::Ident(k) if k == "for" => {
                self.bump();
                self.expect_p("(")?;
                let subj = self.npat()?;
                self.expect_p(")")?;
                let body = self.sp_atom()?;
                Ok(SpAst::In { subj, body: Box::new(body) })
            }
            Tok::P("<") => {
                let subj = self.npat()?;
                self.expect_p("!")?;
                if self.eat_p("(") {
                    let payload = self.sp()?;
                    self.expect_p(",")?;
                    let pt = self.patid()?;
                    self.expect_p(",")?;
                    let carry = self.patid()?;
                    self.expect_p(")")?;
                    Ok(SpAst::Out { subj, payload: Box::new(payload), ptimbre: pt, carry })
                } else {
                    let payload = self.sp_atom()?;
                    Ok(SpAst::Out { subj, payload: Box::new(payload), ptimbre: PatId::Any, carry: PatId::Any })
                }
            }
            t => self.err(format!("expected a spatial formula, found {t}")),
        }
    }
    /// `?` matches any component, including a wildcard; `_` matches only a
    /// wildcard.
    fn patid(&mut self) -> R<PatId> {
        if self.eat_p("?") {
            Ok(PatId::Any)
        } else if self.eat_p("_") {
            Ok(PatId::Wild)
        } else {
            Ok(PatId::Id(self.ident()?))
        }
    }
    fn npat(&mut self) -> R<NPat> {
        self.expect_p("<")?;
        let quote = if self.eat_p("?") {
            None
        } else if self.is_p("_") {
            return self.err("a location is never a wildcard: write `?` for any location");
        } else {
            Some(Box::new(self.sp()?))
        };
        self.expect_p(",")?;
        let timbre = self.patid()?;
        self.expect_p(",")?;
        let datum = self.patid()?;
        self.expect_p(">")?;
        Ok(NPat { quote, timbre, datum })
    }

    // -------------------------------------------------------- configuration

    fn cfg(&mut self) -> R<CfgAst> {
        let mut v = vec![self.cfg_and()?];
        while self.eat_p("||") {
            v.push(self.cfg_and()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { CfgAst::Or(v) })
    }
    fn cfg_and(&mut self) -> R<CfgAst> {
        let mut v = vec![self.cfg_atom()?];
        while self.eat_p("&&") {
            v.push(self.cfg_atom()?);
        }
        Ok(if v.len() == 1 { v.pop().unwrap() } else { CfgAst::And(v) })
    }
    fn cfg_atom(&mut self) -> R<CfgAst> {
        match self.peek().clone() {
            Tok::P("!") => {
                self.bump();
                Ok(CfgAst::Not(Box::new(self.cfg_atom()?)))
            }
            Tok::P("(") => {
                self.bump();
                let c = self.cfg()?;
                self.expect_p(")")?;
                Ok(c)
            }
            Tok::P("<>") => {
                self.bump();
                Ok(CfgAst::Dia { guard: None, body: Box::new(self.cfg_atom()?) })
            }
            Tok::P("<") => {
                self.bump();
                let g = self.crisp()?;
                self.expect_p(">")?;
                Ok(CfgAst::Dia { guard: Some(Box::new(g)), body: Box::new(self.cfg_atom()?) })
            }
            Tok::Ident(k) => match k.as_str() {
                "true" => {
                    self.bump();
                    Ok(CfgAst::True)
                }
                "false" => {
                    self.bump();
                    Ok(CfgAst::False)
                }
                "live" => {
                    self.bump();
                    Ok(CfgAst::Live)
                }
                "nu" => {
                    self.bump();
                    let x = self.ident()?;
                    self.expect_p(".")?;
                    let b = self.cfg()?;
                    Ok(CfgAst::Nu(x, Box::new(b)))
                }
                "mu" => self.err("least fixed points are not in the checkable fragment of this player"),
                _ => Ok(CfgAst::Var(self.ident()?)),
            },
            t => self.err(format!("expected a configuration formula, found {t}")),
        }
    }
}

/// Parse a canonical machine exported by the instrument:
/// `machine N (uniform | start S) { s -> t @ p/q, ... }` with state indices.
pub fn parse_machine_file(file: &str, src: &str) -> Result<MachineBody, Diag> {
    let mut nf = NoFiles;
    let mut p = Parser {
        toks: lex(file, src)?,
        pos: 0,
        file: file.into(),
        sigs: HashMap::new(),
        name_defs: HashSet::new(),
        params: HashMap::new(),
        factors: vec![],
        loader: &mut nf,
        imported: vec![],
        out: File::default(),
        no_seq: 0,
    };
    p.expect_kw("machine")?;
    let n = p.int()? as usize;
    if p.eat_kw("start") {
        p.int()?;
    } else {
        p.expect_kw("uniform")?;
    }
    p.expect_p("{")?;
    let edges = p.sep_list("}", |q| {
        let s = q.int()? as usize;
        q.expect_p("->")?;
        let t = q.int()? as usize;
        q.expect_p("@")?;
        let w = q.number()?;
        if s >= n || t >= n {
            return q.err(format!("state index out of range for a machine on {n} states"));
        }
        Ok((s, t, w))
    })?;
    Ok(MachineBody::Indexed { n, edges })
}
