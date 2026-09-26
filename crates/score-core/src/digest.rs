//! Structural, id-independent digests of terms (digest subsection).
//!
//! Ids depend on interning order, so they are never serialised as
//! identities. The structural digest of a process is SHA-256 over a canonical
//! encoding in which the children of a parallel composition are ordered by
//! their own digests. Labels (stream-key annotations) are excluded.

use crate::alphabet::Timbre;
use crate::sha256::Sha256;
use crate::term::{Arena, ClauseId, Name, Proc, ProcId};
use std::collections::HashMap;

pub type Digest = [u8; 32];

/// Something that can digest a clause structurally.
pub trait ClauseDigests {
    fn clause_digest(&self, c: ClauseId) -> Digest;
}

#[derive(Default)]
pub struct Digester {
    memo: HashMap<ProcId, Digest>,
    trunc: HashMap<(ProcId, u32), Digest>,
}

fn put_timbre(h: &mut Sha256, t: Timbre) {
    h.update(&t.0.to_be_bytes());
}

impl Digester {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn proc(&mut self, a: &Arena, cl: &dyn ClauseDigests, p: ProcId) -> Digest {
        if let Some(d) = self.memo.get(&p) {
            return *d;
        }
        // iterative on deep chains would be nicer; records are shallow enough
        let mut h = Sha256::new();
        match a.get(p) {
            Proc::Nil => h.update(b"N"),
            Proc::Par(cs) => {
                h.update(b"P");
                h.update(&(cs.len() as u32).to_be_bytes());
                let mut ds: Vec<Digest> = cs.iter().map(|&c| self.proc(a, cl, c)).collect();
                ds.sort();
                for d in ds {
                    h.update(&d);
                }
            }
            Proc::Recv { subjects, clause, body, .. } => {
                h.update(b"R");
                h.update(&(subjects.len() as u32).to_be_bytes());
                for n in subjects.iter() {
                    self.name(a, cl, *n, &mut h);
                }
                h.update(&cl.clause_digest(*clause));
                let b = self.proc(a, cl, *body);
                h.update(&b);
            }
            Proc::Send { subj, payload, ptimbre, carry } => {
                h.update(b"S");
                self.name(a, cl, *subj, &mut h);
                let d = self.proc(a, cl, *payload);
                h.update(&d);
                put_timbre(&mut h, *ptimbre);
                h.update(&carry.0.to_be_bytes());
            }
            Proc::Drop(n) => {
                h.update(b"D");
                self.name(a, cl, *n, &mut h);
            }
        }
        let d = h.finish();
        self.memo.insert(p, d);
        d
    }

    fn name(&mut self, a: &Arena, cl: &dyn ClauseDigests, n: Name, h: &mut Sha256) {
        match n {
            Name::Var(i) => {
                h.update(b"v");
                h.update(&i.to_be_bytes());
            }
            Name::Lvl(l) => {
                h.update(b"l");
                h.update(&l.to_be_bytes());
            }
            Name::Quote { proc, timbre, datum } => {
                h.update(b"q");
                let d = self.proc(a, cl, proc);
                h.update(&d);
                put_timbre(h, timbre);
                h.update(&datum.0.to_be_bytes());
            }
        }
    }

    /// A digest of `p` in which everything below `depth` levels of nesting
    /// (through message payloads, the quotes of names, and receipt bodies) is
    /// erased. Two processes agreeing on this digest satisfy the same spatial
    /// formulae of depth at most `depth`.
    pub fn truncated(&mut self, a: &Arena, cl: &dyn ClauseDigests, p: ProcId, depth: u32) -> Digest {
        if depth == 0 {
            return [0u8; 32];
        }
        if let Some(d) = self.trunc.get(&(p, depth)) {
            return *d;
        }
        let mut h = Sha256::new();
        h.update(b"T");
        match a.get(p) {
            Proc::Nil => h.update(b"N"),
            Proc::Par(cs) => {
                h.update(b"P");
                let mut ds: Vec<Digest> = cs.iter().map(|&c| self.truncated(a, cl, c, depth)).collect();
                ds.sort();
                for d in ds {
                    h.update(&d);
                }
            }
            Proc::Recv { subjects, clause, body, .. } => {
                h.update(b"R");
                for n in subjects.iter() {
                    self.tname(a, cl, *n, depth - 1, &mut h);
                }
                h.update(&cl.clause_digest(*clause));
                let b = self.truncated(a, cl, *body, depth - 1);
                h.update(&b);
            }
            Proc::Send { subj, payload, ptimbre, carry } => {
                h.update(b"S");
                self.tname(a, cl, *subj, depth - 1, &mut h);
                let d = self.truncated(a, cl, *payload, depth - 1);
                h.update(&d);
                put_timbre(&mut h, *ptimbre);
                h.update(&carry.0.to_be_bytes());
            }
            Proc::Drop(n) => {
                h.update(b"D");
                self.tname(a, cl, *n, depth, &mut h);
            }
        }
        let d = h.finish();
        self.trunc.insert((p, depth), d);
        d
    }

    fn tname(&mut self, a: &Arena, cl: &dyn ClauseDigests, n: Name, depth: u32, h: &mut Sha256) {
        match n {
            Name::Quote { proc, timbre, datum } => {
                h.update(b"q");
                let d = self.truncated(a, cl, proc, depth);
                h.update(&d);
                put_timbre(h, timbre);
                h.update(&datum.0.to_be_bytes());
            }
            Name::Var(i) => {
                h.update(b"v");
                h.update(&i.to_be_bytes());
            }
            Name::Lvl(l) => {
                h.update(b"l");
                h.update(&l.to_be_bytes());
            }
        }
    }
}
