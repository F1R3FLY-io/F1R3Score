//! `score-core`: the data alphabets, hash-consed terms in congruence normal
//! form, substitution, names and locations of the score calculus
//! (*Scores as Processes*, `publications/f1r3score`). No I/O.

pub mod alphabet;
pub mod base;
pub mod digest;
pub mod hat;
pub mod q;
pub mod record;
pub mod sha256;
pub mod term;

pub use alphabet::{Alphabets, Datum, DurDecl, PitchDecl, Polarity, Timbre, TimbreDecl};
pub use q::Q;
pub use hat::{subjects_match, Deco, Hat};
pub use record::{nu, PlainNote, Record};
pub use term::{instance_of, Arena, ClauseId, Label, Name, Proc, ProcId};

/// A location: the quote of a process. Communication happens only between a
/// receipt and a message at equivalent locations; the timbre is part of each
/// subject's decoration, not of the location (R-locindex).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Loc {
    pub quote: ProcId,
}
