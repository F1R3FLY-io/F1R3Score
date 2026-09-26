//! `score-core`: the data alphabets, hash-consed terms in congruence normal
//! form, substitution, names and locations of the score calculus
//! (*Scores as Processes*, `publications/f1r3score`). No I/O.

pub mod alphabet;
pub mod base;
pub mod digest;
pub mod q;
pub mod sha256;
pub mod term;

pub use alphabet::{Alphabets, Datum, DurDecl, PitchDecl, Polarity, Timbre, TimbreDecl};
pub use q::Q;
pub use term::{Arena, ClauseId, Label, Name, Proc, ProcId};

/// A location: the quote of a process together with a timbre. Communication
/// happens only between a receipt and a message at the same location.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Loc {
    pub quote: ProcId,
    pub timbre: Timbre,
}
