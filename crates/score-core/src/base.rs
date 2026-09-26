//! Bases: closed receipts on a dead channel, tagged by a string.
//!
//! `base "Lm"` elaborates to `for(_ <- <@T, tau_bot, r>) 0` where `T` encodes
//! the tag in the core syntax itself, so that distinct tags give inequivalent
//! bases and the elaborated term is a closed term of the core syntax and
//! nothing else. A byte `b` is the numeral `Z(b)` (`Z(0) = 0`,
//! `Z(n+1) = <@Z(n), tau_bot, r>!(0, tau_bot, r)`), and a string is the list
//! `L("") = 0`, `L(b s) = <@Z(b), tau_bot, r>!(L(s), tau_bot, r)`. Only the
//! reserved timbre is used, so none of it can ever communicate.

use crate::alphabet::{Datum, Timbre};
use crate::hat::Hat;
use crate::term::{Arena, ClauseId, Name, Proc, ProcId};

const DEAD: Hat<Timbre> = Hat::Is(Timbre::DEAD);

fn numeral(a: &mut Arena, n: u8, rest: Datum) -> ProcId {
    let mut z = a.nil();
    for _ in 0..n {
        let nil = a.nil();
        z = a.send(Name::Quote { proc: z, timbre: DEAD, datum: Hat::Is(rest) }, nil, DEAD, Hat::Is(rest));
    }
    z
}

/// The list encoding `L(s)` of a string in the core syntax.
pub fn tag_list(a: &mut Arena, tag: &str, rest: Datum) -> ProcId {
    let mut l = a.nil();
    for &b in tag.as_bytes().iter().rev() {
        let z = numeral(a, b, rest);
        l = a.send(Name::Quote { proc: z, timbre: DEAD, datum: Hat::Is(rest) }, l, DEAD, Hat::Is(rest));
    }
    l
}

pub fn base(a: &mut Arena, tag: &str, rest: Datum) -> ProcId {
    let l = tag_list(a, tag, rest);
    let nil = a.nil();
    a.recv(vec![Name::Quote { proc: l, timbre: DEAD, datum: Hat::Is(rest) }], ClauseId::TRUE, nil, None)
}

/// The key location `K_{P,k} = <@P, tau_bot, k>!(0, _, _)` of key `k` on the
/// keyboard whose base is `P` (Section 9.1). Distinct pairs give inequivalent
/// locations; it is a message, but not record-shaped (its payload is `0`, not
/// `P`), so freshness collection never touches it.
pub fn keyloc(a: &mut Arena, p: ProcId, k: Datum) -> ProcId {
    let nil = a.nil();
    a.send(Name::Quote { proc: p, timbre: DEAD, datum: Hat::Is(k) }, nil, Hat::Wild, Hat::Wild)
}

/// The code location `C_{P,k,tau} = <@P, tau_bot, k>!(0, tau, _)` of the key
/// of timbre `tau` at `K_{P,k}`: distinct from the key location (its payload
/// timbre is concrete) and from every other timbre's code location.
pub fn keycode(a: &mut Arena, p: ProcId, k: Datum, tau: Timbre) -> ProcId {
    let nil = a.nil();
    a.send(Name::Quote { proc: p, timbre: DEAD, datum: Hat::Is(k) }, nil, Hat::Is(tau), Hat::Wild)
}

/// The acknowledgement location `A_n = <@L(n), kappa, eps>!(0, kappa, eps)` of
/// the `n`-th synchronous-output site (Def. 9.3). Built on the control
/// timbre, which user code cannot write, so it can collide with nothing a
/// score writes; not record-shaped.
pub fn ackloc(a: &mut Arena, n: u64, rest: Datum, eps: Datum) -> ProcId {
    let l = tag_list(a, &n.to_string(), rest);
    let nil = a.nil();
    let ctl = Hat::Is(Timbre::CTL);
    a.send(Name::Quote { proc: l, timbre: ctl, datum: Hat::Is(eps) }, nil, ctl, Hat::Is(eps))
}

fn decode_numeral(a: &Arena, mut p: ProcId) -> Option<u8> {
    let mut n: u32 = 0;
    loop {
        match a.get(p) {
            Proc::Nil => return u8::try_from(n).ok(),
            Proc::Send { subj: Name::Quote { proc, timbre: DEAD, .. }, payload, ptimbre: DEAD, .. }
                if matches!(a.get(*payload), Proc::Nil) =>
            {
                n += 1;
                p = *proc;
            }
            _ => return None,
        }
    }
}

/// If `l` is a list encoding `L(s)`, the string.
pub fn decode_tag_list(a: &Arena, mut l: ProcId) -> Option<String> {
    let mut bytes = vec![];
    loop {
        match a.get(l) {
            Proc::Nil => break,
            Proc::Send { subj: Name::Quote { proc, timbre: DEAD, .. }, payload, ptimbre: DEAD, .. } => {
                bytes.push(decode_numeral(a, *proc)?);
                l = *payload;
            }
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
}

/// If `p` is a key location `K_{P,k}`, its keyboard base and key.
pub fn decode_keyloc(a: &Arena, p: ProcId) -> Option<(ProcId, Datum)> {
    match a.get(p) {
        Proc::Send { subj: Name::Quote { proc, timbre: DEAD, datum: Hat::Is(k) }, payload, ptimbre: Hat::Wild, carry: Hat::Wild }
            if matches!(a.get(*payload), Proc::Nil) =>
        {
            Some((*proc, *k))
        }
        _ => None,
    }
}

/// If `p` is a key code location `C_{P,k,tau}`, its base, key and timbre.
pub fn decode_keycode(a: &Arena, p: ProcId) -> Option<(ProcId, Datum, Timbre)> {
    match a.get(p) {
        Proc::Send {
            subj: Name::Quote { proc, timbre: DEAD, datum: Hat::Is(k) },
            payload,
            ptimbre: Hat::Is(t),
            carry: Hat::Wild,
        } if matches!(a.get(*payload), Proc::Nil) => Some((*proc, *k, *t)),
        _ => None,
    }
}

/// If `p` is an acknowledgement location `A_n`, its index.
pub fn decode_ackloc(a: &Arena, p: ProcId) -> Option<u64> {
    let ctl = Hat::Is(Timbre::CTL);
    match a.get(p) {
        Proc::Send { subj: Name::Quote { proc, timbre, .. }, payload, ptimbre, .. }
            if *timbre == ctl && *ptimbre == ctl && matches!(a.get(*payload), Proc::Nil) =>
        {
            decode_tag_list(a, *proc)?.parse().ok()
        }
        _ => None,
    }
}

/// If `p` is a base, its tag.
pub fn decode_base(a: &Arena, p: ProcId) -> Option<String> {
    let Proc::Recv { subjects, clause, body, .. } = a.get(p) else { return None };
    if subjects.len() != 1 || *clause != ClauseId::TRUE || !matches!(a.get(*body), Proc::Nil) {
        return None;
    }
    let Name::Quote { proc: l, timbre: DEAD, .. } = subjects[0] else { return None };
    decode_tag_list(a, l)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bases_roundtrip_and_differ() {
        let mut a = Arena::new();
        let r = Datum(0);
        let b1 = base(&mut a, "Lm", r);
        let b2 = base(&mut a, "Lb", r);
        let b3 = base(&mut a, "Lm", r);
        assert_eq!(b1, b3);
        assert_ne!(b1, b2);
        assert_eq!(decode_base(&a, b1).as_deref(), Some("Lm"));
        assert_eq!(decode_base(&a, b2).as_deref(), Some("Lb"));
        assert!(a.is_closed(b1));
    }

    #[test]
    fn key_and_code_locations_differ_and_are_not_record_shaped() {
        let mut a = Arena::new();
        let (r, k1, k2) = (Datum(0), Datum(1), Datum(2));
        let p = base(&mut a, "P1", r);
        let q = base(&mut a, "P2", r);
        let locs = [
            keyloc(&mut a, p, k1),
            keyloc(&mut a, p, k2),
            keyloc(&mut a, q, k1),
            keycode(&mut a, p, k1, Timbre(0)),
            keycode(&mut a, p, k1, Timbre(1)),
            ackloc(&mut a, 1, r, Datum(3)),
            ackloc(&mut a, 2, r, Datum(3)),
        ];
        for (i, x) in locs.iter().enumerate() {
            assert!(!a.is_record_shaped(*x));
            for y in &locs[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }
}
