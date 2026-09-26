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
use crate::term::{Arena, ClauseId, Name, Proc, ProcId};

fn numeral(a: &mut Arena, n: u8, rest: Datum) -> ProcId {
    let mut z = a.nil();
    for _ in 0..n {
        let nil = a.nil();
        z = a.send(Name::Quote { proc: z, timbre: Timbre::DEAD, datum: rest }, nil, Timbre::DEAD, rest);
    }
    z
}

pub fn base(a: &mut Arena, tag: &str, rest: Datum) -> ProcId {
    let mut l = a.nil();
    for &b in tag.as_bytes().iter().rev() {
        let z = numeral(a, b, rest);
        l = a.send(Name::Quote { proc: z, timbre: Timbre::DEAD, datum: rest }, l, Timbre::DEAD, rest);
    }
    let nil = a.nil();
    a.recv(vec![Name::Quote { proc: l, timbre: Timbre::DEAD, datum: rest }], ClauseId::TRUE, nil, None)
}

fn decode_numeral(a: &Arena, mut p: ProcId) -> Option<u8> {
    let mut n: u32 = 0;
    loop {
        match a.get(p) {
            Proc::Nil => return u8::try_from(n).ok(),
            Proc::Send { subj: Name::Quote { proc, timbre: Timbre::DEAD, .. }, payload, ptimbre: Timbre::DEAD, .. }
                if matches!(a.get(*payload), Proc::Nil) =>
            {
                n += 1;
                p = *proc;
            }
            _ => return None,
        }
    }
}

/// If `p` is a base, its tag.
pub fn decode_base(a: &Arena, p: ProcId) -> Option<String> {
    let Proc::Recv { subjects, clause, body, .. } = a.get(p) else { return None };
    if subjects.len() != 1 || *clause != ClauseId::TRUE || !matches!(a.get(*body), Proc::Nil) {
        return None;
    }
    let Name::Quote { proc: mut l, timbre: Timbre::DEAD, .. } = subjects[0] else { return None };
    let mut bytes = vec![];
    loop {
        match a.get(l) {
            Proc::Nil => break,
            Proc::Send { subj: Name::Quote { proc, timbre: Timbre::DEAD, .. }, payload, ptimbre: Timbre::DEAD, .. } => {
                bytes.push(decode_numeral(a, *proc)?);
                l = *payload;
            }
            _ => return None,
        }
    }
    String::from_utf8(bytes).ok()
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
}
