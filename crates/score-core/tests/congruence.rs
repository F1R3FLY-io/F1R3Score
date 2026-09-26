//! Congruence property test: interning agrees with an independent decision
//! procedure on 10^4 random term pairs, including `*@P` and alpha-variants.

use score_core::*;

#[derive(Clone, Debug)]
enum T {
    Nil,
    Par(Vec<T>),
    Recv(N, String, Box<T>),
    Send(N, Box<T>, u16),
    Drop(N),
}
#[derive(Clone, Debug)]
enum N {
    Var(String),
    Quote(Box<T>, u16),
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn gen(r: &mut Rng, depth: u32, scope: &mut Vec<String>) -> T {
    let k = if depth == 0 { r.below(2) } else { r.below(5) };
    match k {
        0 => T::Nil,
        1 if !scope.is_empty() => T::Drop(N::Var(scope[r.below(scope.len() as u64) as usize].clone())),
        1 => T::Nil,
        2 => T::Par((0..1 + r.below(3)).map(|_| gen(r, depth - 1, scope)).collect()),
        3 => {
            let x = gen_name(r, depth - 1, scope);
            let y = format!("y{}", r.below(4));
            scope.push(y.clone());
            let b = gen(r, depth - 1, scope);
            scope.pop();
            T::Recv(x, y, Box::new(b))
        }
        _ => {
            let x = gen_name(r, depth - 1, scope);
            T::Send(x, Box::new(gen(r, depth - 1, scope)), r.below(3) as u16)
        }
    }
}
fn gen_name(r: &mut Rng, depth: u32, scope: &mut Vec<String>) -> N {
    if !scope.is_empty() && r.below(2) == 0 {
        N::Var(scope[r.below(scope.len() as u64) as usize].clone())
    } else {
        N::Quote(Box::new(gen(r, depth, scope)), r.below(3) as u16)
    }
}

/// A congruent variant: permute parallel components, add `0`s, wrap
/// components in `*<@P, t, d>`, and rename binders.
fn variant(r: &mut Rng, t: &T, ren: &mut Vec<(String, String)>) -> T {
    let out = match t {
        T::Nil => T::Nil,
        T::Par(v) => {
            let mut w: Vec<T> = v.iter().map(|x| variant(r, x, ren)).collect();
            for i in (1..w.len()).rev() {
                let j = r.below(i as u64 + 1) as usize;
                w.swap(i, j);
            }
            if r.below(2) == 0 {
                w.push(T::Nil);
            }
            T::Par(w)
        }
        T::Recv(x, y, b) => {
            let x2 = vname(r, x, ren);
            let y2 = format!("{y}_{}", r.below(1000));
            ren.push((y.clone(), y2.clone()));
            let b2 = variant(r, b, ren);
            ren.pop();
            T::Recv(x2, y2, Box::new(b2))
        }
        T::Send(x, p, d) => T::Send(vname(r, x, ren), Box::new(variant(r, p, ren)), *d),
        T::Drop(x) => T::Drop(vname(r, x, ren)),
    };
    if r.below(4) == 0 {
        T::Drop(N::Quote(Box::new(out), r.below(3) as u16))
    } else {
        out
    }
}
fn vname(r: &mut Rng, n: &N, ren: &mut Vec<(String, String)>) -> N {
    match n {
        N::Var(v) => N::Var(ren.iter().rev().find(|(a, _)| a == v).map(|x| x.1.clone()).unwrap_or(v.clone())),
        N::Quote(p, d) => N::Quote(Box::new(variant(r, p, ren)), *d),
    }
}

// ------------------------------------------------ the arena construction

const TB: Timbre = Timbre(0);

fn build(a: &mut Arena, t: &T, env: &mut Vec<String>) -> ProcId {
    match t {
        T::Nil => a.nil(),
        T::Par(v) => {
            let cs: Vec<ProcId> = v.iter().map(|x| build(a, x, env)).collect();
            a.par(cs)
        }
        T::Recv(x, y, b) => {
            let n = bname(a, x, env);
            let l = env.len() as u32;
            env.push(y.clone());
            let body = build(a, b, env);
            env.pop();
            let c = a.close(body, l, 1);
            a.recv(vec![n], ClauseId::TRUE, c, None)
        }
        T::Send(x, p, d) => {
            let n = bname(a, x, env);
            let pl = build(a, p, env);
            a.send(n, pl, TB, Datum(*d))
        }
        T::Drop(x) => {
            let n = bname(a, x, env);
            a.drop_name(n)
        }
    }
}
fn bname(a: &mut Arena, n: &N, env: &mut Vec<String>) -> Name {
    match n {
        N::Var(v) => Name::Lvl(env.iter().rposition(|x| x == v).unwrap() as u32),
        N::Quote(p, d) => {
            let q = build(a, p, env);
            Name::Quote { proc: q, timbre: TB, datum: Datum(*d) }
        }
    }
}

// ------------------------------------ the independent decision procedure

/// A canonical string: de Bruijn indices, flattened and sorted parallel
/// components, units removed, and `*<@P,..>` replaced by P.
fn canon(t: &T, env: &mut Vec<String>) -> Vec<String> {
    match t {
        T::Nil => vec![],
        T::Par(v) => {
            let mut out: Vec<String> = v.iter().flat_map(|x| canon(x, env)).collect();
            out.sort();
            out
        }
        T::Recv(x, y, b) => {
            let n = cname(x, env);
            env.push(y.clone());
            let body = join(canon(b, env));
            env.pop();
            vec![format!("R[{n}]{{{body}}}")]
        }
        T::Send(x, p, d) => vec![format!("S[{}]({}){d}", cname(x, env), join(canon(p, env)))],
        T::Drop(N::Quote(p, _)) => canon(p, env),
        T::Drop(N::Var(v)) => vec![format!("D{}", env.len() - 1 - env.iter().rposition(|x| x == v).unwrap())],
    }
}
fn join(mut v: Vec<String>) -> String {
    v.sort();
    format!("<{}>", v.join("|"))
}
fn cname(n: &N, env: &mut Vec<String>) -> String {
    match n {
        N::Var(v) => format!("v{}", env.len() - 1 - env.iter().rposition(|x| x == v).unwrap()),
        N::Quote(p, d) => format!("q{}{d}", join(canon(p, env))),
    }
}

#[test]
fn interning_decides_congruence() {
    let mut r = Rng(0x9E3779B97F4A7C15);
    let mut a = Arena::new();
    let (mut eq, mut ne) = (0, 0);
    for _ in 0..10_000 {
        let t1 = gen(&mut r, 4, &mut vec![]);
        let t2 = match r.below(3) {
            0 => variant(&mut r, &t1, &mut vec![]),
            1 => gen(&mut r, 4, &mut vec![]),
            _ => {
                // a near miss: a variant with one datum or one binder use changed
                let v = variant(&mut r, &t1, &mut vec![]);
                match v {
                    T::Send(x, p, d) => T::Send(x, p, (d + 1) % 3),
                    other => T::Par(vec![other, T::Send(N::Quote(Box::new(T::Nil), 0), Box::new(T::Nil), 0)]),
                }
            }
        };
        let (p1, p2) = (build(&mut a, &t1, &mut vec![]), build(&mut a, &t2, &mut vec![]));
        let (c1, c2) = (join(canon(&t1, &mut vec![])), join(canon(&t2, &mut vec![])));
        assert_eq!(p1 == p2, c1 == c2, "disagreement:\n{t1:?}\n{t2:?}\n{c1}\n{c2}");
        if c1 == c2 {
            eq += 1
        } else {
            ne += 1
        }
    }
    assert!(eq > 3000 && ne > 3000, "both outcomes exercised: {eq} equal, {ne} different");
}
