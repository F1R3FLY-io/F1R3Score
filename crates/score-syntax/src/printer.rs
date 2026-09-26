//! The pretty-printer: prints elaborated terms in the core surface syntax
//! (plus `base "tag"`), such that printing and re-parsing is the identity on
//! interned terms.

use score_core::base::{decode_ackloc, decode_base, decode_keycode, decode_keyloc};
use score_core::*;
use score_logic::formula::*;
use score_logic::ClauseArena;
use std::fmt::Write;

pub struct Printer<'a> {
    pub arena: &'a Arena,
    pub clauses: &'a ClauseArena,
    pub alph: &'a Alphabets,
    digester: std::cell::RefCell<score_core::digest::Digester>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

impl<'a> Printer<'a> {
    pub fn new(arena: &'a Arena, clauses: &'a ClauseArena, alph: &'a Alphabets) -> Self {
        Printer { arena, clauses, alph, digester: Default::default() }
    }

    pub fn score(&self, name: Option<&str>, initial: &[(Q, ProcId)]) -> String {
        let mut o = String::new();
        if let Some(n) = name {
            let _ = writeln!(o, "score {n}");
        }
        let ps: Vec<String> = self
            .alph
            .pitches
            .iter()
            .map(|p| match p.midi {
                Some(m) if p.name != "r" => format!("{} = {m}", p.name),
                _ => p.name.clone(),
            })
            .collect();
        let _ = writeln!(o, "pitches {{ {} }}", ps.join(", "));
        let ds: Vec<String> = self
            .alph
            .durations
            .iter()
            .filter(|d| d.name != "eps")
            .map(|d| format!("{} = {}", d.name, d.len))
            .collect();
        let _ = writeln!(o, "durations {{ {} }}", ds.join(", "));
        let ts: Vec<String> = self
            .alph
            .timbres
            .iter()
            .map(|t| match t.program {
                Some(g) => format!("{} = gm({g}) on {}", t.name, t.channel),
                None => format!("{} on {}", t.name, t.channel),
            })
            .collect();
        let _ = writeln!(o, "timbres {{ {} }}", ts.join(", "));
        let items: Vec<String> = initial
            .iter()
            .map(|(t, p)| {
                let body = self.proc_str(*p);
                if t.is_zero() {
                    format!("({body})")
                } else {
                    format!("at {t} {{ {body} }}")
                }
            })
            .collect();
        let _ = writeln!(o, "play {}", if items.is_empty() { "0".to_string() } else { items.join("\n   | ") });
        o
    }

    pub fn proc_str(&self, p: ProcId) -> String {
        let mut s = String::new();
        self.proc(p, &mut vec![], &mut s, true);
        s
    }

    fn proc(&self, p: ProcId, names: &mut Vec<String>, o: &mut String, top: bool) {
        if let Some(tag) = decode_base(self.arena, p) {
            let _ = write!(o, "base \"{}\"", esc(&tag));
            return;
        }
        if let Some((b, k)) = decode_keyloc(self.arena, p) {
            o.push_str("keyloc(");
            self.proc(b, names, o, true);
            let _ = write!(o, ", {})", self.alph.name(k));
            return;
        }
        if let Some((b, k, t)) = decode_keycode(self.arena, p) {
            o.push_str("keycode(");
            self.proc(b, names, o, true);
            let _ = write!(o, ", {}, {})", self.alph.name(k), self.alph.timbre_name(t));
            return;
        }
        if let Some(n) = decode_ackloc(self.arena, p) {
            let _ = write!(o, "ackloc({n})");
            return;
        }
        match self.arena.get(p) {
            Proc::Nil => o.push('0'),
            Proc::Par(cs) => {
                if !top {
                    o.push('(');
                }
                // print children in structural-digest order, so that output
                // does not depend on interning order
                let mut cs: Vec<ProcId> = cs.to_vec();
                {
                    let mut dg = self.digester.borrow_mut();
                    cs.sort_by_key(|c| dg.proc(self.arena, self.clauses, *c));
                }
                for (i, c) in cs.iter().enumerate() {
                    if i > 0 {
                        o.push_str(" | ");
                    }
                    self.proc(*c, names, o, false);
                }
                if !top {
                    o.push(')');
                }
            }
            Proc::Recv { subjects, clause, body, label } => {
                o.push_str("for ");
                if let Some(l) = label {
                    let _ = write!(o, "#{} ", self.arena.label_name(*l));
                }
                o.push('(');
                let base = names.len();
                let k = subjects.len();
                let fresh: Vec<String> = (0..k).map(|i| format!("y{}", base + i)).collect();
                for (i, n) in subjects.iter().enumerate() {
                    if i > 0 {
                        o.push_str(" & ");
                    }
                    let _ = write!(o, "{} <- ", fresh[i]);
                    self.name(*n, names, o);
                }
                if *clause != ClauseId::TRUE {
                    o.push_str(" where ");
                    o.push_str(&self.clause_str(*clause));
                }
                o.push_str(") { ");
                for i in (0..k).rev() {
                    names.push(fresh[i].clone());
                }
                self.proc(*body, names, o, true);
                names.truncate(base);
                o.push_str(" }");
            }
            Proc::Send { subj, payload, ptimbre, carry } => {
                self.name(*subj, names, o);
                o.push_str("!(");
                self.proc(*payload, names, o, true);
                if ptimbre.is_wild() && carry.is_wild() {
                    o.push(')');
                } else {
                    let _ = write!(o, ", {}, {})", self.hat_t(*ptimbre), self.hat_d(*carry));
                }
            }
            Proc::Drop(n) => {
                o.push('*');
                self.name(*n, names, o);
            }
        }
    }

    fn name(&self, n: Name, names: &[String], o: &mut String) {
        match n {
            Name::Var(i) => {
                let i = i as usize;
                if i < names.len() {
                    o.push_str(&names[names.len() - 1 - i]);
                } else {
                    let _ = write!(o, "free{i}");
                }
            }
            Name::Lvl(l) => {
                let _ = write!(o, "lvl{l}");
            }
            Name::Quote { proc, timbre, datum } => {
                o.push_str("<@");
                let mut names2 = names.to_vec();
                self.proc(proc, &mut names2, o, false);
                let _ = write!(o, ", {}, {}>", self.hat_t(timbre), self.hat_d(datum));
            }
        }
    }

    fn hat_t(&self, t: Hat<Timbre>) -> String {
        match t {
            Hat::Is(t) => self.alph.timbre_name(t).to_string(),
            Hat::Wild => "_".into(),
        }
    }
    fn hat_d(&self, d: Hat<Datum>) -> String {
        match d {
            Hat::Is(d) => self.alph.name(d).to_string(),
            Hat::Wild => "_".into(),
        }
    }
    fn pat_t(&self, t: &PatC<Timbre>) -> String {
        match t {
            PatC::Any => "?".into(),
            PatC::Wild => "_".into(),
            PatC::Is(t) => self.alph.timbre_name(*t).to_string(),
        }
    }
    fn pat_d(&self, d: &PatC<Datum>) -> String {
        match d {
            PatC::Any => "?".into(),
            PatC::Wild => "_".into(),
            PatC::Is(d) => self.alph.name(*d).to_string(),
        }
    }

    // --------------------------------------------------------------- clauses

    pub fn clause_str(&self, c: ClauseId) -> String {
        self.graded(self.clauses.get(c), true)
    }

    fn graded(&self, g: &Graded, top: bool) -> String {
        match g {
            Graded::Const(q) => q.to_string(),
            Graded::Crisp(c) => format!("[{}]", self.crisp(c)),
            Graded::Tensor(v) => {
                let s = v.iter().map(|x| self.graded(x, false)).collect::<Vec<_>>().join(" * ");
                if v.len() < 2 {
                    format!("({})", if v.is_empty() { "1".into() } else { s })
                } else {
                    format!("({s})")
                }
            }
            Graded::Sum(v) => {
                if v.is_empty() {
                    return "0".into();
                }
                let s = v.iter().map(|x| self.graded(x, false)).collect::<Vec<_>>().join(" + ");
                if top && v.len() > 1 {
                    s
                } else {
                    format!("({s})")
                }
            }
            Graded::Table(t) => self.table(t),
        }
    }

    fn keyfn(&self, k: &KeyFn) -> String {
        let slot = |s: &u8| if *s == 0 { String::new() } else { format!("#{}", s + 1) };
        match k {
            KeyFn::Pitch(s) => format!("pitch{}", slot(s)),
            KeyFn::Dur(s) => format!("dur{}", slot(s)),
            KeyFn::Carry(s) => format!("carry{}", slot(s)),
            KeyFn::Prev => "prev".into(),
            KeyFn::Held => "held".into(),
            KeyFn::Step(s) => format!("step{}", slot(s)),
        }
    }
    fn keyval(&self, v: &KeyVal) -> String {
        match v {
            KeyVal::D(d) => self.alph.name(*d).to_string(),
            KeyVal::I(i) => i.to_string(),
            KeyVal::Undef | KeyVal::Pad => "_".into(),
        }
    }

    fn table(&self, t: &Table) -> String {
        let n = t.key.len();
        let key = if n == 1 {
            self.keyfn(&t.key[0])
        } else {
            format!("({})", t.key.iter().map(|k| self.keyfn(k)).collect::<Vec<_>>().join(", "))
        };
        let mut es: Vec<String> = t
            .entries
            .iter()
            .map(|(k, w)| {
                let ks: Vec<String> = k[..n].iter().map(|v| self.keyval(v)).collect();
                if n == 1 {
                    format!("{}: {w}", ks[0])
                } else {
                    format!("({}): {w}", ks.join(", "))
                }
            })
            .collect();
        es.push(format!("_: {}", t.default));
        format!("table {key} {{ {} }}", es.join(", "))
    }

    fn crisp(&self, c: &Crisp) -> String {
        match c {
            Crisp::True => "true".into(),
            Crisp::False => "false".into(),
            Crisp::Not(a) => format!("!({})", self.crisp(a)),
            Crisp::And(v) => {
                if v.is_empty() {
                    "true".into()
                } else {
                    format!("({})", v.iter().map(|x| self.crisp(x)).collect::<Vec<_>>().join(" && "))
                }
            }
            Crisp::Or(v) => {
                if v.is_empty() {
                    "false".into()
                } else {
                    format!("({})", v.iter().map(|x| self.crisp(x)).collect::<Vec<_>>().join(" || "))
                }
            }
            Crisp::Atom(a) => self.atom(a),
            Crisp::Leads(cfg) => format!("leads({})", self.cfg(cfg)),
        }
    }

    fn atom(&self, a: &Atom) -> String {
        let slot = |s: &u8| if *s == 0 { String::new() } else { format!("#{}", s + 1) };
        match a {
            Atom::Pitch(s, d) => format!("pitch{}({})", slot(s), self.alph.name(*d)),
            Atom::Dur(s, d) => format!("dur{}({})", slot(s), self.alph.name(*d)),
            Atom::Carry(s, d) => format!("carry{}({})", slot(s), self.hat_d(*d)),
            Atom::Step(s, k) => format!("step{}({k})", slot(s)),
            Atom::At(sp) => format!("at({})", self.sp(sp)),
            Atom::Passes(s, sp) => format!("passes{}({})", slot(s), self.sp(sp)),
        }
    }

    fn sp(&self, s: &Sp) -> String {
        match s {
            Sp::True => "true".into(),
            Sp::Zero => "0".into(),
            Sp::Not(a) => format!("!({})", self.sp(a)),
            Sp::And(v) => {
                if v.is_empty() {
                    "true".into()
                } else {
                    format!("({})", v.iter().map(|x| self.sp(x)).collect::<Vec<_>>().join(" && "))
                }
            }
            Sp::Or(v) => {
                if v.is_empty() {
                    "!(true)".into()
                } else {
                    format!("({})", v.iter().map(|x| self.sp(x)).collect::<Vec<_>>().join(" || "))
                }
            }
            Sp::Par(a, b) => format!("({} | {})", self.sp(a), self.sp(b)),
            Sp::Out { subj, payload, ptimbre, carry } => format!(
                "{}!({}, {}, {})",
                self.npat(subj),
                self.sp(payload),
                self.pat_t(ptimbre),
                self.pat_d(carry)
            ),
            Sp::In { subj, body } => format!("for({}) ({})", self.npat(subj), self.sp(body)),
        }
    }

    fn npat(&self, n: &NamePat) -> String {
        format!(
            "<{}, {}, {}>",
            n.quote.as_ref().map(|q| self.sp(q)).unwrap_or("?".into()),
            self.pat_t(&n.timbre),
            self.pat_d(&n.datum)
        )
    }

    fn cfg(&self, c: &Cfg) -> String {
        match c {
            Cfg::True => "true".into(),
            Cfg::False => "false".into(),
            Cfg::Live => "live".into(),
            Cfg::Not(a) => format!("!({})", self.cfg(a)),
            Cfg::And(v) => format!("({})", v.iter().map(|x| self.cfg(x)).collect::<Vec<_>>().join(" && ")),
            Cfg::Or(v) => format!("({})", v.iter().map(|x| self.cfg(x)).collect::<Vec<_>>().join(" || ")),
            Cfg::Dia { guard: None, body } => format!("<> ({})", self.cfg(body)),
            Cfg::Dia { guard: Some(g), body } => format!("<{}> ({})", self.crisp(g), self.cfg(body)),
            Cfg::Nu(x, b) => format!("(nu X{x}. {})", self.cfg(b)),
            Cfg::Var(x) => format!("X{x}"),
        }
    }
}
