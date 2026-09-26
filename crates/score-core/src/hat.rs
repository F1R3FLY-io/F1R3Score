//! Decoration components that may be the wildcard (Definition 2.1 and the
//! matching relation, Definition 2.4 of the note).

use crate::alphabet::{Alphabets, Datum, Polarity, Timbre};

/// A decoration component: concrete, or the wildcard `_`.
///
/// `Hat::Wild` is equal only to `Hat::Wild` (name equivalence, Def. 2.3); the
/// relation that lets a wildcard stand for a concrete value is [`Hat::meet`],
/// used by matching and by nothing else (Rem. 2.6).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Hat<T> {
    Is(T),
    Wild,
}

impl<T: Copy + Eq> Hat<T> {
    /// The meet of two matching components: the concrete one if either is
    /// concrete, `_` if both are `_`; `None` when they do not match.
    pub fn meet(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Hat::Wild, x) | (x, Hat::Wild) => Some(x),
            (Hat::Is(a), Hat::Is(b)) if a == b => Some(Hat::Is(a)),
            _ => None,
        }
    }
    pub fn is_wild(self) -> bool {
        matches!(self, Hat::Wild)
    }
    pub fn concrete(self) -> Option<T> {
        match self {
            Hat::Is(x) => Some(x),
            Hat::Wild => None,
        }
    }
    /// `self` is an instance of `general` in this component: `general` is `_`
    /// or equal to `self`.
    pub fn instance_of(self, general: Self) -> bool {
        general.is_wild() || self == general
    }
}

impl Hat<Datum> {
    /// The polarity of a name with this datum; a wildcard datum has none.
    pub fn polarity(self, alph: &Alphabets) -> Option<Polarity> {
        self.concrete().map(|d| alph.polarity(d))
    }
}

/// The subject decoration of a receipt pattern or a message.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Deco {
    pub timbre: Hat<Timbre>,
    pub datum: Hat<Datum>,
}

/// Do a receipt's subject and a message's subject (at equivalent locations)
/// match (Defs. 2.4 and 2.7)? Their timbres must meet in a concrete timbre and
/// their data must both be concrete and of opposite polarity. Returns the
/// concrete meet, which is the note's timbre.
pub fn subjects_match(recv: Deco, send: Deco, alph: &Alphabets) -> Option<Timbre> {
    let t = recv.timbre.meet(send.timbre)?.concrete()?;
    let (a, b) = (recv.datum.concrete()?, send.datum.concrete()?);
    if alph.is_pitch(a) == alph.is_pitch(b) {
        return None;
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alphabet::{DurDecl, PitchDecl, TimbreDecl};
    use crate::q::Q;

    fn alph() -> Alphabets {
        Alphabets::new(
            vec![PitchDecl { name: "r".into(), midi: None }, PitchDecl { name: "C4".into(), midi: Some(60) }],
            vec![DurDecl { name: "q".into(), len: Q::new(1, 4) }],
            vec![
                TimbreDecl { name: "piano".into(), program: None, channel: 1 },
                TimbreDecl { name: "vibes".into(), program: None, channel: 2 },
            ],
        )
        .unwrap()
    }
    fn deco(t: Hat<Timbre>, d: Hat<Datum>) -> Deco {
        Deco { timbre: t, datum: d }
    }

    #[test]
    fn matching_on_subjects() {
        let a = alph();
        let (c4, q) = (Hat::Is(a.lookup_datum("C4").unwrap()), Hat::Is(a.lookup_datum("q").unwrap()));
        let (piano, vibes) = (Hat::Is(Timbre(0)), Hat::Is(Timbre(1)));
        // concrete and equal timbres
        assert_eq!(subjects_match(deco(piano, c4), deco(piano, q), &a), Some(Timbre(0)));
        // an open timbre against a concrete one: the note is in the concrete timbre
        assert_eq!(subjects_match(deco(piano, c4), deco(Hat::Wild, q), &a), Some(Timbre(0)));
        assert_eq!(subjects_match(deco(Hat::Wild, c4), deco(vibes, q), &a), Some(Timbre(1)));
        // both open: never
        assert_eq!(subjects_match(deco(Hat::Wild, c4), deco(Hat::Wild, q), &a), None);
        // different concrete timbres: never
        assert_eq!(subjects_match(deco(piano, c4), deco(vibes, q), &a), None);
        // an open datum on either subject: no polarity, never
        assert_eq!(subjects_match(deco(piano, Hat::Wild), deco(piano, q), &a), None);
        assert_eq!(subjects_match(deco(piano, c4), deco(piano, Hat::Wild), &a), None);
        // equal polarities: never
        assert_eq!(subjects_match(deco(piano, c4), deco(piano, c4), &a), None);
        assert_eq!(subjects_match(deco(piano, q), deco(piano, q), &a), None);
        // the dual rule (COMM2): the receipt holds the duration
        assert_eq!(subjects_match(deco(piano, q), deco(Hat::Wild, c4), &a), Some(Timbre(0)));
    }

    #[test]
    fn playback_reads_the_note_off_the_record() {
        use crate::record::{nu, Record};
        use crate::term::ProcId;
        let a = alph();
        let (c4, q) = (a.lookup_datum("C4").unwrap(), a.lookup_datum("q").unwrap());
        let r = Record {
            loc: ProcId(0),
            recv: deco(Hat::Wild, Hat::Is(q)),
            send: deco(Hat::Is(Timbre(1)), Hat::Is(c4)),
            payload: ProcId(0),
            // the payload decoration takes no part: it may name another timbre
            ptimbre: Hat::Is(Timbre(0)),
            carry: Hat::Wild,
        };
        let n = nu(&r, &a).unwrap();
        assert_eq!((n.timbre, n.pitch, n.dur), (Timbre(1), c4, q));
    }
}
