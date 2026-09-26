//! Data alphabets (R-data, R-alph).
//!
//! A datum is an index into the disjoint union of the pitch alphabet and the
//! duration alphabet. Pitches come first, in declaration order (the rest `r`
//! wherever it is declared); durations follow in declaration order with the
//! implicit null duration `eps` last.

use crate::q::Q;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Datum(pub u16);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Timbre(pub u16);

impl Timbre {
    /// The reserved timbre no message may use (tau_bot); bases listen on it.
    pub const DEAD: Timbre = Timbre(u16::MAX);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Polarity {
    /// the datum is a pitch: the name is a channel
    Channel,
    /// the datum is a duration: the name is a co-channel
    CoChannel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PitchDecl {
    pub name: String,
    pub midi: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurDecl {
    pub name: String,
    pub len: Q,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimbreDecl {
    pub name: String,
    /// General MIDI program (0-based), if declared
    pub program: Option<u8>,
    /// MIDI channel, 1-based as written
    pub channel: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alphabets {
    pub pitches: Vec<PitchDecl>,
    pub durations: Vec<DurDecl>,
    pub timbres: Vec<TimbreDecl>,
    rest: u16,
    eps: u16,
    /// ordered position of each pitch (None for the rest)
    ord: Vec<Option<u16>>,
    ordered: Vec<Datum>,
    positive: Vec<Datum>,
}

pub const REST_NAME: &str = "r";
pub const EPS_NAME: &str = "eps";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlphabetError(pub String);

impl Alphabets {
    /// Build from declarations. `durations` must not contain `eps`; it is added.
    pub fn new(
        pitches: Vec<PitchDecl>,
        mut durations: Vec<DurDecl>,
        timbres: Vec<TimbreDecl>,
    ) -> Result<Alphabets, AlphabetError> {
        let mut seen = BTreeMap::new();
        for (kind, n) in pitches
            .iter()
            .map(|p| ("pitch", &p.name))
            .chain(durations.iter().map(|d| ("duration", &d.name)))
            .chain(timbres.iter().map(|t| ("timbre", &t.name)))
        {
            if n == EPS_NAME && kind == "duration" {
                return Err(AlphabetError("`eps = 0` is implicit and must not be redeclared".into()));
            }
            if let Some(k) = seen.insert(n.clone(), kind) {
                return Err(AlphabetError(format!(
                    "`{n}` is declared as both a {k} and a {kind}; the alphabets must be pairwise distinct"
                )));
            }
        }
        if seen.contains_key(EPS_NAME) {
            return Err(AlphabetError("`eps` is reserved for the null duration".into()));
        }
        let rest = pitches
            .iter()
            .position(|p| p.name == REST_NAME)
            .ok_or_else(|| AlphabetError("the pitch alphabet must contain the rest `r`".into()))?;
        if pitches.len() + durations.len() + 1 >= u16::MAX as usize {
            return Err(AlphabetError("alphabet too large".into()));
        }
        if timbres.len() >= u16::MAX as usize {
            return Err(AlphabetError("too many timbres".into()));
        }
        for p in &pitches {
            if p.name != REST_NAME && p.midi.is_none() {
                return Err(AlphabetError(format!(
                    "pitch `{}` is not in scientific notation and needs an explicit MIDI number (`{} = 60`)",
                    p.name, p.name
                )));
            }
        }
        durations.push(DurDecl { name: EPS_NAME.into(), len: Q::ZERO });
        let np = pitches.len();
        let mut ord = vec![None; np];
        let mut ordered = vec![];
        for i in 0..pitches.len() {
            if i != rest {
                ord[i] = Some(ordered.len() as u16);
                ordered.push(Datum(i as u16));
            }
        }
        let eps = (np + durations.len() - 1) as u16;
        let positive = durations
            .iter()
            .enumerate()
            .filter(|(_, d)| d.len.is_positive())
            .map(|(i, _)| Datum((np + i) as u16))
            .collect();
        Ok(Alphabets { pitches, durations, timbres, rest: rest as u16, eps, ord, ordered, positive })
    }

    pub fn rest(&self) -> Datum {
        Datum(self.rest)
    }
    pub fn eps(&self) -> Datum {
        Datum(self.eps)
    }
    pub fn n_pitches(&self) -> usize {
        self.pitches.len()
    }
    pub fn n_data(&self) -> usize {
        self.pitches.len() + self.durations.len()
    }
    pub fn is_pitch(&self, d: Datum) -> bool {
        (d.0 as usize) < self.pitches.len()
    }
    pub fn polarity(&self, d: Datum) -> Polarity {
        if self.is_pitch(d) {
            Polarity::Channel
        } else {
            Polarity::CoChannel
        }
    }
    /// Position among the ordered pitches (the rest is unordered).
    pub fn ord(&self, d: Datum) -> Option<i64> {
        if self.is_pitch(d) {
            self.ord[d.0 as usize].map(|x| x as i64)
        } else {
            None
        }
    }
    /// Length in whole notes; pitches have none.
    pub fn len(&self, d: Datum) -> Q {
        if self.is_pitch(d) {
            Q::ZERO
        } else {
            self.durations[d.0 as usize - self.pitches.len()].len.clone()
        }
    }
    pub fn midi(&self, d: Datum) -> Option<u8> {
        if self.is_pitch(d) {
            self.pitches[d.0 as usize].midi
        } else {
            None
        }
    }
    pub fn name(&self, d: Datum) -> &str {
        let i = d.0 as usize;
        if i < self.pitches.len() {
            &self.pitches[i].name
        } else {
            &self.durations[i - self.pitches.len()].name
        }
    }
    pub fn timbre_name(&self, t: Timbre) -> &str {
        if t == Timbre::DEAD {
            "<dead>"
        } else {
            &self.timbres[t.0 as usize].name
        }
    }
    pub fn lookup_datum(&self, name: &str) -> Option<Datum> {
        if let Some(i) = self.pitches.iter().position(|p| p.name == name) {
            return Some(Datum(i as u16));
        }
        self.durations
            .iter()
            .position(|d| d.name == name)
            .map(|i| Datum((self.pitches.len() + i) as u16))
    }
    pub fn lookup_timbre(&self, name: &str) -> Option<Timbre> {
        self.timbres.iter().position(|t| t.name == name).map(|i| Timbre(i as u16))
    }
    /// All pitches in declaration order (the `pitches` generator).
    pub fn all_pitches(&self) -> Vec<Datum> {
        (0..self.pitches.len()).map(|i| Datum(i as u16)).collect()
    }
    /// The ordered pitches (the `pitches-r` generator).
    pub fn ordered_pitches(&self) -> &[Datum] {
        &self.ordered
    }
    /// All durations including `eps` (the `durations` generator).
    pub fn all_durations(&self) -> Vec<Datum> {
        (0..self.durations.len()).map(|i| Datum((self.pitches.len() + i) as u16)).collect()
    }
    /// Durations of positive length (the `durations+` generator).
    pub fn positive_durations(&self) -> &[Datum] {
        &self.positive
    }
    pub fn all_timbres(&self) -> Vec<Timbre> {
        (0..self.timbres.len()).map(|i| Timbre(i as u16)).collect()
    }
}

/// MIDI number of a pitch in scientific notation: `C4` = 60, `F#3`, `Bb2`, `C-1` = 0.
pub fn scientific_midi(name: &str) -> Option<u8> {
    let mut cs = name.chars().peekable();
    let letter = cs.next()?;
    let pc: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut acc = 0i32;
    while let Some(&c) = cs.peek() {
        match c {
            '#' => acc += 1,
            'b' => acc -= 1,
            _ => break,
        }
        cs.next();
    }
    let rest: String = cs.collect();
    if rest.is_empty() {
        return None;
    }
    let oct: i32 = rest.parse().ok()?;
    let m = 12 * (oct + 1) + pc + acc;
    if (0..=127).contains(&m) {
        Some(m as u8)
    } else {
        None
    }
}

const SHARP_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

/// Name a MIDI number in scientific notation.
pub fn midi_name(m: u8, flats: bool) -> String {
    let names = if flats { &FLAT_NAMES } else { &SHARP_NAMES };
    format!("{}{}", names[(m % 12) as usize], (m as i32) / 12 - 1)
}

/// Expand `scale KIND LO .. HI` to pitch declarations (inclusive).
pub fn expand_scale(kind: &str, lo: &str, hi: &str) -> Result<Vec<PitchDecl>, String> {
    let steps: &[u8] = match kind {
        "major" | "ionian" => &[0, 2, 4, 5, 7, 9, 11],
        "minor" | "aeolian" => &[0, 2, 3, 5, 7, 8, 10],
        "dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "pentatonic" | "major-pentatonic" => &[0, 2, 4, 7, 9],
        "minor-pentatonic" => &[0, 3, 5, 7, 10],
        "blues" => &[0, 3, 5, 6, 7, 10],
        "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        _ => return Err(format!("unknown scale `{kind}`")),
    };
    let a = scientific_midi(lo).ok_or_else(|| format!("`{lo}` is not a pitch in scientific notation"))?;
    let b = scientific_midi(hi).ok_or_else(|| format!("`{hi}` is not a pitch in scientific notation"))?;
    if b < a {
        return Err(format!("empty scale range {lo} .. {hi}"));
    }
    let flats = lo.contains('b') && lo.len() > 1 && lo.as_bytes()[1] == b'b';
    let tonic = a % 12;
    let mut out = vec![];
    for m in a..=b {
        let rel = (12 + m % 12 - tonic) % 12;
        if steps.contains(&rel) {
            out.push(PitchDecl { name: midi_name(m, flats), midi: Some(m) });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midi_numbers() {
        assert_eq!(scientific_midi("C4"), Some(60));
        assert_eq!(scientific_midi("F#3"), Some(54));
        assert_eq!(scientific_midi("Gb3"), Some(54));
        assert_eq!(scientific_midi("Bb2"), Some(46));
        assert_eq!(scientific_midi("A4"), Some(69));
        assert_eq!(scientific_midi("deg7"), None);
    }
    #[test]
    fn scale() {
        let s = expand_scale("major", "C4", "C5").unwrap();
        let n: Vec<_> = s.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(n, ["C4", "D4", "E4", "F4", "G4", "A4", "B4", "C5"]);
        assert_eq!(expand_scale("pentatonic", "C4", "C7").unwrap().len(), 16);
    }
}
