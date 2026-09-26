//! The window-behavioural fragment: configuration formulae (`leads`, `<g>`,
//! `nu`) decided on the finite graph of configurations reachable by single
//! communications, with stamps erased, inert record locations collected, and
//! states identified by their contents with every quote truncated to the
//! score's window depth (Decision "the actual company"; R-fragment).
//!
//! Soundness rests on two conditions, both enforced: every clause of the
//! score reads the past to depth at most the window (true by construction of
//! the window), and locations are fresh -- a collected location is never
//! re-entered (a spawn at a tombstone is an error, not an approximation).
//! A state bound makes an oversized graph an error, never a truncation.

use crate::soup::{Cand, LocalWeigher, Soup};
use crate::EngineError;
use score_core::digest::Digester;
use score_core::sha256::Sha256;
use score_core::*;
use score_logic::{Cfg, ClauseArena, Crisp, Evaluator, LocalOnly, SlotView, View};
use std::collections::{BTreeSet, HashMap};

struct Edge {
    loc: Loc,
    slots: Vec<SlotView>,
    succ: usize,
}

struct Node {
    soup: Option<Soup>,
    edges: Option<Vec<Edge>>,
}

pub struct Explorer {
    pub window: u32,
    pub max_states: usize,
    pub max_alternatives: usize,
    nodes: Vec<Node>,
    index: HashMap<[u8; 32], usize>,
    digester: Digester,
    cache: HashMap<Cfg, (usize, Vec<bool>)>,
}

impl Explorer {
    pub fn new(window: u32, max_states: usize, max_alternatives: usize) -> Explorer {
        Explorer {
            window: window.max(1),
            max_states,
            max_alternatives,
            nodes: vec![],
            index: HashMap::new(),
            digester: Digester::new(),
            cache: HashMap::new(),
        }
    }

    pub fn states(&self) -> usize {
        self.nodes.len()
    }

    /// Collect record locations (in `which`, or everywhere) that hold no
    /// locally available candidate.
    pub fn prune(
        soup: &mut Soup,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
        which: Option<&BTreeSet<Loc>>,
        bound: usize,
    ) -> Result<(), EngineError> {
        let locs: Vec<Loc> = match which {
            Some(w) => w.iter().copied().collect(),
            None => soup.locs.keys().copied().collect(),
        };
        for l in locs {
            if !soup.locs.contains_key(&l) || !Soup::is_record(arena, l) {
                continue;
            }
            let c = soup.candidates(arena, clauses, alph, l, &mut LocalWeigher, bound, false)?;
            if c.is_empty() {
                soup.collect(l);
            }
        }
        Ok(())
    }

    fn key(&mut self, soup: &Soup, arena: &Arena, clauses: &ClauseArena) -> [u8; 32] {
        let d = self.window;
        let mut entries: Vec<[u8; 32]> = vec![];
        for (l, st) in &soup.locs {
            if st.recvs.is_empty() && st.sends.is_empty() {
                continue;
            }
            let mut h = Sha256::new();
            h.update(&self.digester.truncated(arena, clauses, l.quote, d));
            let mut rs: Vec<Vec<u8>> = st
                .recvs
                .iter()
                .map(|r| {
                    let mut v = vec![b'r'];
                    v.extend_from_slice(format!("{:?}", r.subjects).as_bytes());
                    v.extend_from_slice(&r.clause.0.to_be_bytes());
                    v.extend_from_slice(&self.digester.truncated(arena, clauses, r.body, d + 1));
                    v
                })
                .collect();
            let mut ss: Vec<Vec<u8>> = st
                .sends
                .iter()
                .map(|s| {
                    let mut v = vec![b's'];
                    v.extend_from_slice(format!("{:?}", s.subj).as_bytes());
                    v.extend_from_slice(&self.digester.truncated(arena, clauses, s.payload, d + 1));
                    v.extend_from_slice(format!("{:?}{:?}", s.ptimbre, s.carry).as_bytes());
                    v
                })
                .collect();
            rs.sort();
            ss.sort();
            for x in rs.iter().chain(ss.iter()) {
                h.update(x);
            }
            entries.push(h.finish());
        }
        entries.sort();
        let mut h = Sha256::new();
        for e in entries {
            h.update(&e);
        }
        h.finish()
    }

    fn node_for(&mut self, soup: Soup, arena: &Arena, clauses: &ClauseArena) -> Result<usize, EngineError> {
        let k = self.key(&soup, arena, clauses);
        if let Some(&n) = self.index.get(&k) {
            return Ok(n);
        }
        if self.nodes.len() >= self.max_states {
            return Err(EngineError::new(
                "window-graph-too-large",
                format!(
                    "the window graph of a behavioural formula exceeds {} states; raise --max-states or \
                     shorten the window (the player does not approximate)",
                    self.max_states
                ),
            ));
        }
        let n = self.nodes.len();
        self.nodes.push(Node { soup: Some(soup), edges: None });
        self.index.insert(k, n);
        Ok(n)
    }

    fn expand_from(&mut self, n0: usize, arena: &mut Arena, clauses: &ClauseArena, alph: &Alphabets) -> Result<(), EngineError> {
        let mut work = vec![n0];
        while let Some(n) = work.pop() {
            if self.nodes[n].edges.is_some() {
                continue;
            }
            let soup = self.nodes[n].soup.take().expect("unexpanded node keeps its configuration");
            let mut edges = vec![];
            let locs: Vec<Loc> = soup.locs.keys().copied().collect();
            for l in locs {
                let cands = soup.candidates(arena, clauses, alph, l, &mut LocalWeigher, self.max_alternatives, false)?;
                for c in cands {
                    let mut s2 = soup.clone();
                    let mut touched = BTreeSet::new();
                    s2.fire(arena, clauses, alph, l, std::slice::from_ref(&c), &mut touched)?;
                    Self::prune(&mut s2, arena, clauses, alph, Some(&touched), self.max_alternatives)?;
                    let m = self.node_for(s2, arena, clauses)?;
                    if self.nodes[m].edges.is_none() {
                        work.push(m);
                    }
                    edges.push(Edge { loc: l, slots: c.slots, succ: m });
                }
            }
            self.nodes[n].edges = Some(edges);
        }
        Ok(())
    }

    /// Does the configuration (already pruned) satisfy the formula?
    pub fn holds(
        &mut self,
        config: Soup,
        cfg: &Cfg,
        arena: &mut Arena,
        clauses: &ClauseArena,
        alph: &Alphabets,
    ) -> Result<bool, EngineError> {
        let n0 = self.node_for(config, arena, clauses)?;
        self.expand_from(n0, arena, clauses, alph)?;
        let count = self.nodes.len();
        if let Some((c, v)) = self.cache.get(cfg) {
            if *c == count {
                return Ok(v[n0]);
            }
        }
        let mut env: Vec<(u32, Vec<bool>)> = vec![];
        let v = self.eval(cfg, &mut env, arena, alph)?;
        let r = v[n0];
        self.cache.insert(cfg.clone(), (count, v));
        Ok(r)
    }

    fn guard(&self, g: &Crisp, e: &Edge, arena: &Arena, alph: &Alphabets) -> Result<bool, EngineError> {
        let mut ev = Evaluator::new(arena, alph);
        let view = View { timbre: e.slots[0].timbre, loc: e.loc.quote, slots: &e.slots };
        let r = ev.crisp(g, &view, &mut LocalOnly);
        if let Some(err) = ev.error {
            return Err(EngineError::eval(err));
        }
        Ok(r)
    }

    fn eval(&self, c: &Cfg, env: &mut Vec<(u32, Vec<bool>)>, arena: &Arena, alph: &Alphabets) -> Result<Vec<bool>, EngineError> {
        let n = self.nodes.len();
        let edges = |i: usize| self.nodes[i].edges.as_deref().unwrap_or(&[]);
        Ok(match c {
            Cfg::True => vec![true; n],
            Cfg::False => vec![false; n],
            Cfg::Live => (0..n).map(|i| !edges(i).is_empty()).collect(),
            Cfg::Not(a) => self.eval(a, env, arena, alph)?.into_iter().map(|x| !x).collect(),
            Cfg::And(v) => {
                let mut acc = vec![true; n];
                for x in v {
                    let r = self.eval(x, env, arena, alph)?;
                    for i in 0..n {
                        acc[i] &= r[i];
                    }
                }
                acc
            }
            Cfg::Or(v) => {
                let mut acc = vec![false; n];
                for x in v {
                    let r = self.eval(x, env, arena, alph)?;
                    for i in 0..n {
                        acc[i] |= r[i];
                    }
                }
                acc
            }
            Cfg::Dia { guard, body } => {
                let b = self.eval(body, env, arena, alph)?;
                let mut out = vec![false; n];
                for (i, o) in out.iter_mut().enumerate() {
                    for e in edges(i) {
                        if b[e.succ] && guard.as_ref().map_or(Ok(true), |g| self.guard(g, e, arena, alph))? {
                            *o = true;
                            break;
                        }
                    }
                }
                out
            }
            Cfg::Nu(x, body) => {
                let mut cur = vec![true; n];
                loop {
                    env.push((*x, cur.clone()));
                    let next = self.eval(body, env, arena, alph);
                    env.pop();
                    let next = next?;
                    if next == cur {
                        break cur;
                    }
                    cur = next;
                }
            }
            Cfg::Var(x) => env
                .iter()
                .rev()
                .find(|(y, _)| y == x)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| EngineError::eval(format!("unbound fixed-point variable #{x}")))?,
        })
    }
}

/// Fire one candidate in a copy of a configuration (its residue).
pub fn residue(
    config: &Soup,
    loc: Loc,
    c: &Cand,
    arena: &mut Arena,
    clauses: &ClauseArena,
    alph: &Alphabets,
    bound: usize,
) -> Result<Soup, EngineError> {
    let mut s = config.clone();
    let mut touched = BTreeSet::new();
    s.fire(arena, clauses, alph, loc, std::slice::from_ref(c), &mut touched)?;
    Explorer::prune(&mut s, arena, clauses, alph, Some(&touched), bound)?;
    Ok(s)
}
