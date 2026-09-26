//! A minimal JSON value, writer and parser (enough for traces).

use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(i64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj(v: Vec<(&str, J)>) -> J {
        J::Obj(v.into_iter().map(|(k, x)| (k.to_string(), x)).collect())
    }
    pub fn s(x: impl Into<String>) -> J {
        J::Str(x.into())
    }
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(a, _)| a == k).map(|x| &x.1),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            J::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> Option<&[J]> {
        match self {
            J::Arr(v) => Some(v),
            _ => None,
        }
    }
    pub fn write(&self, o: &mut String) {
        match self {
            J::Null => o.push_str("null"),
            J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => {
                let _ = write!(o, "{n}");
            }
            J::Str(s) => {
                o.push('"');
                for c in s.chars() {
                    match c {
                        '"' => o.push_str("\\\""),
                        '\\' => o.push_str("\\\\"),
                        '\n' => o.push_str("\\n"),
                        c if (c as u32) < 0x20 => {
                            let _ = write!(o, "\\u{:04x}", c as u32);
                        }
                        c => o.push(c),
                    }
                }
                o.push('"');
            }
            J::Arr(v) => {
                o.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    x.write(o);
                }
                o.push(']');
            }
            J::Obj(v) => {
                o.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    J::Str(k.clone()).write(o);
                    o.push(':');
                    x.write(o);
                }
                o.push('}');
            }
        }
    }
    pub fn to_string(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
}

pub fn parse(s: &str) -> Result<J, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let v = value(&cs, &mut i)?;
    ws(&cs, &mut i);
    if i != cs.len() {
        return Err(format!("trailing characters at {i}"));
    }
    Ok(v)
}

fn ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

fn value(c: &[char], i: &mut usize) -> Result<J, String> {
    ws(c, i);
    let Some(&ch) = c.get(*i) else { return Err("unexpected end".into()) };
    match ch {
        '{' => {
            *i += 1;
            let mut v = vec![];
            ws(c, i);
            if c.get(*i) == Some(&'}') {
                *i += 1;
                return Ok(J::Obj(v));
            }
            loop {
                ws(c, i);
                let k = match value(c, i)? {
                    J::Str(s) => s,
                    _ => return Err("object keys are strings".into()),
                };
                ws(c, i);
                if c.get(*i) != Some(&':') {
                    return Err(format!("expected ':' at {i}"));
                }
                *i += 1;
                let x = value(c, i)?;
                v.push((k, x));
                ws(c, i);
                match c.get(*i) {
                    Some(',') => *i += 1,
                    Some('}') => {
                        *i += 1;
                        return Ok(J::Obj(v));
                    }
                    _ => return Err(format!("expected ',' or '}}' at {i}")),
                }
            }
        }
        '[' => {
            *i += 1;
            let mut v = vec![];
            ws(c, i);
            if c.get(*i) == Some(&']') {
                *i += 1;
                return Ok(J::Arr(v));
            }
            loop {
                v.push(value(c, i)?);
                ws(c, i);
                match c.get(*i) {
                    Some(',') => *i += 1,
                    Some(']') => {
                        *i += 1;
                        return Ok(J::Arr(v));
                    }
                    _ => return Err(format!("expected ',' or ']' at {i}")),
                }
            }
        }
        '"' => {
            *i += 1;
            let mut s = String::new();
            loop {
                let Some(&d) = c.get(*i) else { return Err("unterminated string".into()) };
                *i += 1;
                match d {
                    '"' => return Ok(J::Str(s)),
                    '\\' => {
                        let e = *c.get(*i).ok_or("bad escape")?;
                        *i += 1;
                        match e {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'u' => {
                                let h: String = c[*i..(*i + 4).min(c.len())].iter().collect();
                                *i += 4;
                                let n = u32::from_str_radix(&h, 16).map_err(|_| "bad \\u escape")?;
                                s.push(char::from_u32(n).unwrap_or('?'));
                            }
                            x => s.push(x),
                        }
                    }
                    x => s.push(x),
                }
            }
        }
        't' | 'f' | 'n' => {
            for (w, v) in [("true", J::Bool(true)), ("false", J::Bool(false)), ("null", J::Null)] {
                let wc: Vec<char> = w.chars().collect();
                if c[*i..].starts_with(&wc) {
                    *i += wc.len();
                    return Ok(v);
                }
            }
            Err(format!("bad literal at {i}"))
        }
        _ => {
            let st = *i;
            while *i < c.len() && (c[*i] == '-' || c[*i].is_ascii_digit()) {
                *i += 1;
            }
            let t: String = c[st..*i].iter().collect();
            t.parse().map(J::Num).map_err(|_| format!("bad number at {st}"))
        }
    }
}

/// Keep BTreeMap import used for callers.
pub type Map = BTreeMap<String, J>;
