//! Hash-consed terms in congruence normal form (R-terms, R-cong, R-nf, R-subst).
//!
//! The arena interns only normal forms: parallel compositions are flattened,
//! `0` is removed, children are sorted (a multiset), singletons unwrapped,
//! `*<@P,t,d>` is replaced by `P`, and bound variables are de Bruijn indices,
//! so that alpha-equivalent terms are identical. Hence `P == Q` (congruence)
//! iff their ids are equal, and location equivalence is integer equality.
//!
//! Two kinds of variable exist. `Var(i)` is a de Bruijn index and is the only
//! kind that appears in elaborated terms. `Lvl(l)` is a de Bruijn *level*
//! used by the elaborator while a binder is open; `close` turns levels into
//! indices when the binder is closed. Levels never reach a run.

use crate::alphabet::{Datum, Timbre};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ProcId(pub u32);

/// Clause identifiers are interned by `score-logic`; `ClauseId::TRUE` is the
/// clause of a receipt written without `where` (the constant 1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ClauseId(pub u32);
impl ClauseId {
    pub const TRUE: ClauseId = ClauseId(0);
}

/// A stream-key annotation (`for #A (...)`). Metadata: it routes chance and is
/// excluded from digests.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Label(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Name {
    /// de Bruijn index
    Var(u32),
    /// de Bruijn level (elaboration only)
    Lvl(u32),
    /// the closed name <@proc, timbre, datum>
    Quote { proc: ProcId, timbre: Timbre, datum: Datum },
}

impl Name {
    pub fn quote(&self) -> Option<(ProcId, Timbre, Datum)> {
        match *self {
            Name::Quote { proc, timbre, datum } => Some((proc, timbre, datum)),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Proc {
    Nil,
    /// flattened, at least two children, sorted by id, no `Nil`
    Par(Box<[ProcId]>),
    /// a receipt (one subject) or a join / chord receipt (several). It binds
    /// one name per subject: inside `body`, the i-th bound name is `Var(i)`.
    Recv { subjects: Box<[Name]>, clause: ClauseId, body: ProcId, label: Option<Label> },
    Send { subj: Name, payload: ProcId, ptimbre: Timbre, carry: Datum },
    /// only on a variable: `*<@P,..>` normalises to `P`
    Drop(Name),
}

pub struct Arena {
    procs: Vec<Proc>,
    map: HashMap<Proc, ProcId>,
    fv: Vec<u32>,
    lv: Vec<u32>,
    labels: Vec<String>,
    label_map: HashMap<String, Label>,
    inst_memo: HashMap<(ProcId, Box<[Name]>), ProcId>,
    close_memo: HashMap<(ProcId, u32, u32), ProcId>,
    relabel_memo: HashMap<(ProcId, Label), ProcId>,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

fn name_fv(n: &Name, fv: &[u32]) -> u32 {
    match n {
        Name::Var(i) => i + 1,
        Name::Lvl(_) => 0,
        Name::Quote { proc, .. } => fv[proc.0 as usize],
    }
}
fn name_lv(n: &Name, lv: &[u32]) -> u32 {
    match n {
        Name::Var(_) => 0,
        Name::Lvl(l) => l + 1,
        Name::Quote { proc, .. } => lv[proc.0 as usize],
    }
}

impl Arena {
    pub fn new() -> Arena {
        let mut a = Arena {
            procs: vec![],
            map: HashMap::new(),
            fv: vec![],
            lv: vec![],
            labels: vec![],
            label_map: HashMap::new(),
            inst_memo: HashMap::new(),
            close_memo: HashMap::new(),
            relabel_memo: HashMap::new(),
        };
        let z = a.intern(Proc::Nil);
        debug_assert_eq!(z, ProcId(0));
        a
    }

    pub fn len(&self) -> usize {
        self.procs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }
    pub fn get(&self, p: ProcId) -> &Proc {
        &self.procs[p.0 as usize]
    }
    /// One more than the largest free de Bruijn index (0 when closed).
    pub fn fv(&self, p: ProcId) -> u32 {
        self.fv[p.0 as usize]
    }
    /// One more than the largest free level (0 when none).
    pub fn lv(&self, p: ProcId) -> u32 {
        self.lv[p.0 as usize]
    }
    pub fn is_closed(&self, p: ProcId) -> bool {
        self.fv(p) == 0 && self.lv(p) == 0
    }
    pub fn name_is_closed(&self, n: &Name) -> bool {
        match n {
            Name::Quote { proc, .. } => self.is_closed(*proc),
            _ => false,
        }
    }

    fn intern(&mut self, p: Proc) -> ProcId {
        if let Some(&id) = self.map.get(&p) {
            return id;
        }
        let (fv, lv) = match &p {
            Proc::Nil => (0, 0),
            Proc::Par(cs) => (
                cs.iter().map(|c| self.fv[c.0 as usize]).max().unwrap_or(0),
                cs.iter().map(|c| self.lv[c.0 as usize]).max().unwrap_or(0),
            ),
            Proc::Recv { subjects, body, .. } => {
                let k = subjects.len() as u32;
                let sf = subjects.iter().map(|n| name_fv(n, &self.fv)).max().unwrap_or(0);
                let sl = subjects.iter().map(|n| name_lv(n, &self.lv)).max().unwrap_or(0);
                (sf.max(self.fv[body.0 as usize].saturating_sub(k)), sl.max(self.lv[body.0 as usize]))
            }
            Proc::Send { subj, payload, .. } => (
                name_fv(subj, &self.fv).max(self.fv[payload.0 as usize]),
                name_lv(subj, &self.lv).max(self.lv[payload.0 as usize]),
            ),
            Proc::Drop(n) => (name_fv(n, &self.fv), name_lv(n, &self.lv)),
        };
        let id = ProcId(self.procs.len() as u32);
        self.procs.push(p.clone());
        self.fv.push(fv);
        self.lv.push(lv);
        self.map.insert(p, id);
        id
    }

    // ---------------------------------------------------------------- formers

    pub fn nil(&self) -> ProcId {
        ProcId(0)
    }

    /// Parallel composition in normal form (a multiset: duplicates are kept).
    pub fn par(&mut self, children: impl IntoIterator<Item = ProcId>) -> ProcId {
        let mut out: Vec<ProcId> = vec![];
        for c in children {
            match self.get(c) {
                Proc::Nil => {}
                Proc::Par(cs) => out.extend(cs.iter().copied()),
                _ => out.push(c),
            }
        }
        match out.len() {
            0 => self.nil(),
            1 => out[0],
            _ => {
                out.sort();
                self.intern(Proc::Par(out.into_boxed_slice()))
            }
        }
    }
    pub fn par2(&mut self, a: ProcId, b: ProcId) -> ProcId {
        self.par([a, b])
    }

    pub fn recv(&mut self, subjects: Vec<Name>, clause: ClauseId, body: ProcId, label: Option<Label>) -> ProcId {
        assert!(!subjects.is_empty());
        self.intern(Proc::Recv { subjects: subjects.into_boxed_slice(), clause, body, label })
    }
    pub fn send(&mut self, subj: Name, payload: ProcId, ptimbre: Timbre, carry: Datum) -> ProcId {
        self.intern(Proc::Send { subj, payload, ptimbre, carry })
    }
    /// `*x`: normalises `*<@P,t,d>` to `P`.
    pub fn drop_name(&mut self, n: Name) -> ProcId {
        match n {
            Name::Quote { proc, .. } => proc,
            _ => self.intern(Proc::Drop(n)),
        }
    }
    pub fn quote(&self, proc: ProcId, timbre: Timbre, datum: Datum) -> Name {
        Name::Quote { proc, timbre, datum }
    }

    /// The parallel components of a normal form (empty for `0`).
    pub fn components(&self, p: ProcId) -> Vec<ProcId> {
        match self.get(p) {
            Proc::Nil => vec![],
            Proc::Par(cs) => cs.to_vec(),
            _ => vec![p],
        }
    }

    // ---------------------------------------------------------------- labels

    pub fn label(&mut self, s: &str) -> Label {
        if let Some(&l) = self.label_map.get(s) {
            return l;
        }
        let l = Label(self.labels.len() as u32);
        self.labels.push(s.to_string());
        self.label_map.insert(s.to_string(), l);
        l
    }
    pub fn label_name(&self, l: Label) -> &str {
        &self.labels[l.0 as usize]
    }

    /// Give every receipt in `p` (through bodies and payloads, not through
    /// the quotes of locations) that has no label the label `l`.
    pub fn relabel(&mut self, p: ProcId, l: Label) -> ProcId {
        if let Some(&r) = self.relabel_memo.get(&(p, l)) {
            return r;
        }
        let r = match self.get(p).clone() {
            Proc::Nil | Proc::Drop(_) => p,
            Proc::Par(cs) => {
                let v: Vec<_> = cs.iter().map(|&c| self.relabel(c, l)).collect();
                self.par(v)
            }
            // receipts on the dead timbre (bases) never run: leave them alone,
            // so that labelling never changes a location
            Proc::Recv { subjects, .. }
                if subjects.iter().any(|n| matches!(n, Name::Quote { timbre: Timbre::DEAD, .. })) =>
            {
                p
            }
            Proc::Recv { subjects, clause, body, label } => {
                let b = self.relabel(body, l);
                self.intern(Proc::Recv { subjects, clause, body: b, label: label.or(Some(l)) })
            }
            Proc::Send { subj, payload, ptimbre, carry } => {
                let pl = self.relabel(payload, l);
                self.send(subj, pl, ptimbre, carry)
            }
        };
        self.relabel_memo.insert((p, l), r);
        r
    }

    // ---------------------------------------------------------- substitution

    fn rewrite(
        &mut self,
        p: ProcId,
        depth: u32,
        memo: &mut HashMap<(ProcId, u32), ProcId>,
        skip: &dyn Fn(&Arena, ProcId, u32) -> bool,
        f: &dyn Fn(Name, u32) -> Name,
    ) -> ProcId {
        if skip(self, p, depth) {
            return p;
        }
        if let Some(&r) = memo.get(&(p, depth)) {
            return r;
        }
        let r = match self.get(p).clone() {
            Proc::Nil => p,
            Proc::Par(cs) => {
                let v: Vec<_> = cs.iter().map(|&c| self.rewrite(c, depth, memo, skip, f)).collect();
                self.par(v)
            }
            Proc::Recv { subjects, clause, body, label } => {
                let k = subjects.len() as u32;
                let subs: Vec<Name> =
                    subjects.iter().map(|n| self.rewrite_name(*n, depth, memo, skip, f)).collect();
                let b = self.rewrite(body, depth + k, memo, skip, f);
                self.recv(subs, clause, b, label)
            }
            Proc::Send { subj, payload, ptimbre, carry } => {
                let s = self.rewrite_name(subj, depth, memo, skip, f);
                let pl = self.rewrite(payload, depth, memo, skip, f);
                self.send(s, pl, ptimbre, carry)
            }
            Proc::Drop(n) => {
                let n2 = self.rewrite_name(n, depth, memo, skip, f);
                self.drop_name(n2)
            }
        };
        memo.insert((p, depth), r);
        r
    }

    fn rewrite_name(
        &mut self,
        n: Name,
        depth: u32,
        memo: &mut HashMap<(ProcId, u32), ProcId>,
        skip: &dyn Fn(&Arena, ProcId, u32) -> bool,
        f: &dyn Fn(Name, u32) -> Name,
    ) -> Name {
        match n {
            Name::Quote { proc, timbre, datum } => {
                let p2 = self.rewrite(proc, depth, memo, skip, f);
                Name::Quote { proc: p2, timbre, datum }
            }
            v => f(v, depth),
        }
    }

    /// `body{names/y}`: replace the bound names of a receipt (indices
    /// `0..k` at the body's top) by closed names. Memoised on (body, names),
    /// since a voice instantiates the same body repeatedly.
    pub fn instantiate(&mut self, body: ProcId, names: &[Name]) -> ProcId {
        let key = (body, names.to_vec().into_boxed_slice());
        if let Some(&r) = self.inst_memo.get(&key) {
            return r;
        }
        debug_assert!(names.iter().all(|n| self.name_is_closed(n)));
        let k = names.len() as u32;
        let ns = names.to_vec();
        let mut memo = HashMap::new();
        let r = self.rewrite(
            body,
            0,
            &mut memo,
            &|a, p, d| a.fv(p) <= d,
            &move |n, d| match n {
                Name::Var(i) if i >= d && i < d + k => ns[(i - d) as usize],
                Name::Var(i) if i >= d + k => Name::Var(i - k),
                other => other,
            },
        );
        self.inst_memo.insert(key, r);
        r
    }

    /// Close `k` levels `L..L+k` of `body` into the indices of a binder placed
    /// directly above it (the i-th level becomes `Var(i)` at the top).
    pub fn close(&mut self, body: ProcId, l: u32, k: u32) -> ProcId {
        if let Some(&r) = self.close_memo.get(&(body, l, k)) {
            return r;
        }
        let mut memo = HashMap::new();
        let r = self.rewrite(
            body,
            0,
            &mut memo,
            &move |a, p, _| a.lv(p) <= l,
            &move |n, d| match n {
                Name::Lvl(x) if x >= l && x < l + k => Name::Var(d + (x - l)),
                other => other,
            },
        );
        self.close_memo.insert((body, l, k), r);
        r
    }

    /// Clear the substitution memo (bounded memory for very long runs).
    pub fn clear_memos(&mut self) {
        self.inst_memo.clear();
        self.relabel_memo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(i: u16) -> Datum {
        Datum(i)
    }
    const T: Timbre = Timbre(0);

    #[test]
    fn par_is_a_multiset() {
        let mut a = Arena::new();
        let n = a.nil();
        let m1 = a.send(a.quote(n, T, d(0)), n, T, d(1));
        let m2 = a.send(a.quote(n, T, d(1)), n, T, d(1));
        let p = a.par([m1, m2, n]);
        let q = a.par([n, m2, m1]);
        assert_eq!(p, q);
        let pp = a.par([p, m1]);
        let qq = a.par([m1, m2, m1]);
        assert_eq!(pp, qq);
        assert_ne!(pp, p, "duplicates are kept");
    }

    #[test]
    fn drop_quote_normalises() {
        let mut a = Arena::new();
        let n = a.nil();
        let m = a.send(a.quote(n, T, d(0)), n, T, d(1));
        let dq = a.drop_name(a.quote(m, T, d(2)));
        assert_eq!(dq, m);
    }

    #[test]
    fn close_then_instantiate() {
        let mut a = Arena::new();
        let n = a.nil();
        // for (y <- <@0,T,0>) { *y | <@*y, T, 1>!(0, T, 1) } written with a level
        let dy = a.drop_name(Name::Lvl(0));
        let s = a.send(Name::Quote { proc: dy, timbre: T, datum: d(1) }, n, T, d(1));
        let body = a.par([dy, s]);
        let closed = a.close(body, 0, 1);
        assert_eq!(a.lv(closed), 0);
        assert_eq!(a.fv(closed), 1);
        let r = a.recv(vec![a.quote(n, T, d(0))], ClauseId::TRUE, closed, None);
        assert!(a.is_closed(r));
        // alpha: the same text closed from a different level is identical
        let dy2 = a.drop_name(Name::Lvl(7));
        let s2 = a.send(Name::Quote { proc: dy2, timbre: T, datum: d(1) }, n, T, d(1));
        let body2 = a.par([dy2, s2]);
        assert_eq!(a.close(body2, 7, 1), closed);
        // instantiate with <@M, T, 2>
        let m = a.send(a.quote(n, T, d(3)), n, T, d(3));
        let inst = a.instantiate(closed, &[a.quote(m, T, d(2))]);
        let s3 = a.send(a.quote(m, T, d(1)), n, T, d(1));
        let expect = a.par([m, s3]);
        assert_eq!(inst, expect);
    }
}
