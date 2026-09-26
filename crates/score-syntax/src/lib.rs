//! `score-syntax`: the textual format of the f1r3score player -- a dialect of
//! the f1r3|@ng control-flow sublanguage with decorated names, alphabet
//! declarations and an elaboration-time macro layer (Section 3 of the spec).
//! No I/O: imports go through the `Loader` trait.

pub mod ast;
pub mod elab;
pub mod lexer;
pub mod parser;
pub mod printer;

pub use elab::{elaborate, Score};
pub use lexer::{Diag, Span};
pub use parser::{parse, Loader, NoFiles, STD_SCORE};
pub use printer::Printer;

/// Parse and elaborate a score.
pub fn load(file: &str, src: &str, loader: &mut dyn Loader) -> Result<Score, Diag> {
    let f = parse(file, src, loader)?;
    elaborate(&f)
}

/// Print an elaborated score in the core syntax (`f1r3score expand`).
pub fn print_score(s: &Score) -> String {
    Printer::new(&s.arena, &s.clauses, &s.alph).score(s.name.as_deref(), &s.initial)
}
