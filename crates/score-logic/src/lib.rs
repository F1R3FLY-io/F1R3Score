//! `score-logic`: the clause language of the f1r3score player -- crisp and
//! graded formula ASTs, the checkable-fragment type, candidate views,
//! evaluation, tables and machines. No I/O.

pub mod arena;
pub mod eval;
pub mod formula;

pub use arena::{Checked, ClauseArena, Locality, Reads, WhyNot};
pub use eval::{Behaviour, Evaluator, LocalOnly, NoBehaviour, SlotView, View};
pub use formula::*;
