//! Records of communications and playback (Definition 2.8 of the note;
//! R-record, R-playback).
//!
//! Reduction is unlabelled. What a communication played is read off the data
//! it matched: the receipt's subject, the message's subject and the payload.
//! [`nu`] is the single playback function; the engine builds candidate views
//! with it and every renderer computes notes with it.

use crate::alphabet::{Alphabets, Datum, Timbre};
use crate::hat::{Deco, Hat};
use crate::term::ProcId;

/// The record of one communication (for a chord receipt, of one of its
/// patterns). The location is shared by both subjects.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Record {
    /// the quote of the location both subjects name
    pub loc: ProcId,
    /// the receipt's subject decoration `<@S, t1, d1>`
    pub recv: Deco,
    /// the message's subject decoration `<@T, t2, d2>`
    pub send: Deco,
    /// the payload `(Q, t3, d)`
    pub payload: ProcId,
    pub ptimbre: Hat<Timbre>,
    pub carry: Hat<Datum>,
}

/// A note: timbre, pitch and duration.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PlainNote {
    pub timbre: Timbre,
    pub pitch: Datum,
    pub dur: Datum,
}

/// Playback: `nu(record) = (t1 meet t2, the pitch, the duration)`. Defined on
/// every record a communication can produce (Prop. 2.10(i)); `None` only on a
/// malformed record, which the engine never emits.
pub fn nu(r: &Record, alph: &Alphabets) -> Option<PlainNote> {
    let timbre = r.recv.timbre.meet(r.send.timbre)?.concrete()?;
    let (d1, d2) = (r.recv.datum.concrete()?, r.send.datum.concrete()?);
    let (pitch, dur) = match (alph.is_pitch(d1), alph.is_pitch(d2)) {
        (true, false) => (d1, d2),
        (false, true) => (d2, d1),
        _ => return None,
    };
    Some(PlainNote { timbre, pitch, dur })
}
