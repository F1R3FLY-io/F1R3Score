//! The lexer. Comments are `//` to end of line and `/* ... */`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}
impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(u64),
    /// a decimal literal such as `0.3`, kept as text to be read exactly
    Dec(String),
    Str(String),
    P(&'static str),
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "`{s}`"),
            Tok::Int(n) => write!(f, "`{n}`"),
            Tok::Dec(s) => write!(f, "`{s}`"),
            Tok::Str(s) => write!(f, "\"{s}\""),
            Tok::P(p) => write!(f, "`{p}`"),
            Tok::Eof => write!(f, "end of file"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub file: String,
    pub span: Span,
    pub msg: String,
}
impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: error: {}", self.file, self.span, self.msg)
    }
}
impl std::error::Error for Diag {}

const PUNCTS: [&str; 30] = [
    "<-", "->", "<>", "..", "++", "&&", "||", "(", ")", "{", "}", "[", "]", "<", ">", ",", ";", ":", "=",
    "!", "|", "&", "*", "+", "-", "/", ".", "@", "#", "_",
];

pub fn lex(file: &str, src: &str) -> Result<Vec<(Tok, Span)>, Diag> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let (mut line, mut col) = (1u32, 1u32);
    let mut out = vec![];
    let adv = |i: &mut usize, line: &mut u32, col: &mut u32, cs: &[char]| {
        if cs[*i] == '\n' {
            *line += 1;
            *col = 1;
        } else {
            *col += 1;
        }
        *i += 1;
    };
    while i < cs.len() {
        let c = cs[i];
        let sp = Span { line, col };
        if c.is_whitespace() {
            adv(&mut i, &mut line, &mut col, &cs);
            continue;
        }
        if c == '/' && i + 1 < cs.len() && cs[i + 1] == '/' {
            while i < cs.len() && cs[i] != '\n' {
                adv(&mut i, &mut line, &mut col, &cs);
            }
            continue;
        }
        if c == '/' && i + 1 < cs.len() && cs[i + 1] == '*' {
            adv(&mut i, &mut line, &mut col, &cs);
            adv(&mut i, &mut line, &mut col, &cs);
            loop {
                if i + 1 >= cs.len() {
                    return Err(Diag { file: file.into(), span: sp, msg: "unterminated comment".into() });
                }
                if cs[i] == '*' && cs[i + 1] == '/' {
                    adv(&mut i, &mut line, &mut col, &cs);
                    adv(&mut i, &mut line, &mut col, &cs);
                    break;
                }
                adv(&mut i, &mut line, &mut col, &cs);
            }
            continue;
        }
        if c == '"' {
            adv(&mut i, &mut line, &mut col, &cs);
            let mut s = String::new();
            loop {
                if i >= cs.len() {
                    return Err(Diag { file: file.into(), span: sp, msg: "unterminated string".into() });
                }
                let d = cs[i];
                adv(&mut i, &mut line, &mut col, &cs);
                match d {
                    '"' => break,
                    '\\' if i < cs.len() => {
                        s.push(cs[i]);
                        adv(&mut i, &mut line, &mut col, &cs);
                    }
                    _ => s.push(d),
                }
            }
            out.push((Tok::Str(s), sp));
            continue;
        }
        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < cs.len() && cs[i].is_ascii_digit() {
                s.push(cs[i]);
                adv(&mut i, &mut line, &mut col, &cs);
            }
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                s.push('.');
                adv(&mut i, &mut line, &mut col, &cs);
                while i < cs.len() && cs[i].is_ascii_digit() {
                    s.push(cs[i]);
                    adv(&mut i, &mut line, &mut col, &cs);
                }
                out.push((Tok::Dec(s), sp));
            } else {
                let n = s.parse::<u64>().map_err(|_| Diag {
                    file: file.into(),
                    span: sp,
                    msg: format!("integer literal `{s}` is too large"),
                })?;
                out.push((Tok::Int(n), sp));
            }
            continue;
        }
        if c.is_ascii_alphabetic() || (c == '_' && i + 1 < cs.len() && cs[i + 1].is_ascii_alphanumeric()) {
            let mut s = String::new();
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_' || cs[i] == '\'') {
                s.push(cs[i]);
                adv(&mut i, &mut line, &mut col, &cs);
            }
            // F#3: a note letter followed by sharps and an octave
            if s.len() == 1
                && "ABCDEFG".contains(s.as_str())
                && i + 1 < cs.len()
                && cs[i] == '#'
                && (cs[i + 1].is_ascii_digit() || cs[i + 1] == '#')
            {
                while i < cs.len() && cs[i] == '#' {
                    s.push('#');
                    adv(&mut i, &mut line, &mut col, &cs);
                }
                while i < cs.len() && cs[i].is_ascii_digit() {
                    s.push(cs[i]);
                    adv(&mut i, &mut line, &mut col, &cs);
                }
            }
            out.push((Tok::Ident(s), sp));
            continue;
        }
        let mut matched = false;
        for p in PUNCTS {
            let pc: Vec<char> = p.chars().collect();
            if i + pc.len() <= cs.len() && cs[i..i + pc.len()] == pc[..] {
                for _ in 0..pc.len() {
                    adv(&mut i, &mut line, &mut col, &cs);
                }
                out.push((Tok::P(p), sp));
                matched = true;
                break;
            }
        }
        if !matched {
            return Err(Diag { file: file.into(), span: sp, msg: format!("unexpected character `{c}`") });
        }
    }
    out.push((Tok::Eof, Span { line, col }));
    Ok(out)
}
