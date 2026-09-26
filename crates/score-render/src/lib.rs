//! `score-render`: performance and trace serialisation, replay parsing, a
//! Standard MIDI File writer, and (feature `live`) live MIDI output.

pub mod json;
pub mod midi;
#[cfg(feature = "live")]
pub mod live;

use json::J;
use score_core::{Alphabets, Q};
use score_engine::{Engine, Event, Note, Recorded};

pub const TRACE_FORMAT: &str = "f1r3score-trace/1";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn note_json(n: &Note, a: &Alphabets) -> J {
    J::Arr(vec![
        J::s(n.onset.to_string()),
        J::s(a.timbre_name(n.timbre)),
        J::s(a.name(n.pitch)),
        J::s(a.name(n.dur)),
    ])
}

/// The performance as a table, sorted by (onset, timbre, pitch, duration).
pub fn performance_table(notes: &[Note], a: &Alphabets) -> String {
    let mut s = format!("{:>10}  {:<10} {:<6} {:<5} {:>8}\n", "onset", "timbre", "pitch", "dur", "length");
    for n in notes {
        s.push_str(&format!(
            "{:>10}  {:<10} {:<6} {:<5} {:>8}\n",
            n.onset.to_string(),
            a.timbre_name(n.timbre),
            a.name(n.pitch),
            a.name(n.dur),
            a.len(n.dur).to_string()
        ));
    }
    s
}

pub fn performance_json(notes: &[Note], a: &Alphabets, name: Option<&str>, digest: &str) -> String {
    let arr = notes
        .iter()
        .map(|n| {
            J::obj(vec![
                ("onset", J::s(n.onset.to_string())),
                ("timbre", J::s(a.timbre_name(n.timbre))),
                ("pitch", J::s(a.name(n.pitch))),
                ("midi", a.midi(n.pitch).map(|m| J::Num(m as i64)).unwrap_or(J::Null)),
                ("dur", J::s(a.name(n.dur))),
                ("length", J::s(a.len(n.dur).to_string())),
            ])
        })
        .collect();
    J::obj(vec![
        ("score", name.map(J::s).unwrap_or(J::Null)),
        ("digest", J::s(digest)),
        ("notes", J::Arr(arr)),
    ])
    .to_string()
}

pub struct Header {
    pub digest: String,
    pub scheduler: String,
    pub mode: String,
    pub gc: bool,
    pub regime: String,
    pub sources: Vec<(String, String)>,
    pub kmax: u32,
}

pub fn trace_header(e: &Engine, digest: &str, file: &str) -> String {
    let a = &e.alph;
    J::obj(vec![
        ("format", J::s(TRACE_FORMAT)),
        ("player", J::s(format!("f1r3score {VERSION}"))),
        ("score", J::s(file)),
        ("digest", J::s(digest)),
        (
            "pitches",
            J::Arr(a.pitches.iter().map(|p| J::Arr(vec![J::s(&p.name), p.midi.map(|m| J::Num(m as i64)).unwrap_or(J::Null)])).collect()),
        ),
        ("durations", J::Arr(a.durations.iter().map(|d| J::Arr(vec![J::s(&d.name), J::s(d.len.to_string())])).collect())),
        ("timbres", J::Arr(a.timbres.iter().map(|t| J::s(&t.name)).collect())),
        ("sources", J::Obj(e.sources.describe().into_iter().map(|(k, v)| (k, J::s(v))).collect())),
        ("scheduler", J::s(e.config.scheduler.spec())),
        ("mode", J::s(if e.config.mode == score_engine::Mode::Max { "max" } else { "one" })),
        ("regime", J::s(e.regime())),
        ("gc", J::s(if e.config.gc { "freshness" } else { "off" })),
        ("kmax", J::Num(e.config.kmax as i64)),
    ])
    .to_string()
}

pub fn trace_event(ev: &Event, a: &Alphabets) -> String {
    J::obj(vec![
        ("step", J::Num(ev.step as i64)),
        ("onset", J::s(ev.onset.to_string())),
        ("loc", J::s(&ev.loc)),
        ("set", J::Num(ev.set_size as i64)),
        ("alts", J::Num(ev.alternatives as i64)),
        ("chosen", J::Num(ev.chosen as i64)),
        ("routing", J::s(&ev.routing)),
        ("digits", J::Obj(ev.digits.iter().map(|(k, d)| (k.clone(), J::Num(*d as i64))).collect())),
        ("notes", J::Arr(ev.notes.iter().map(|n| note_json(n, a)).collect())),
    ])
    .to_string()
}

pub struct Trace {
    pub header: J,
    pub digest: String,
    pub recorded: Vec<Recorded>,
    /// the last step the trace records (replay stops after it)
    pub last_step: Option<u64>,
}

/// Parse a JSONL trace against the score's alphabets.
pub fn parse_trace(text: &str, a: &Alphabets) -> Result<Trace, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = json::parse(lines.next().ok_or("empty trace")?)?;
    if header.get("format").and_then(|x| x.as_str()) != Some(TRACE_FORMAT) {
        return Err(format!("not a {TRACE_FORMAT} trace"));
    }
    let digest = header.get("digest").and_then(|x| x.as_str()).ok_or("trace header has no digest")?.to_string();
    let mut recorded = vec![];
    let mut last_step = None;
    for (i, l) in lines.enumerate() {
        let j = json::parse(l).map_err(|e| format!("trace line {}: {e}", i + 2))?;
        let num = |k: &str| j.get(k).and_then(|x| x.as_i64()).ok_or(format!("trace line {}: missing `{k}`", i + 2));
        last_step = Some(num("step")? as u64);
        let alts = num("alts")? as usize;
        if alts <= 1 {
            continue;
        }
        let mut notes = vec![];
        for n in j.get("notes").and_then(|x| x.as_arr()).unwrap_or(&[]) {
            let f = n.as_arr().ok_or("bad note")?;
            let st = |k: usize| f.get(k).and_then(|x| x.as_str()).ok_or("bad note");
            notes.push(Note {
                onset: Q::parse(st(0)?).ok_or("bad onset")?,
                timbre: a.lookup_timbre(st(1)?).ok_or("unknown timbre in trace")?,
                pitch: a.lookup_datum(st(2)?).ok_or("unknown pitch in trace")?,
                dur: a.lookup_datum(st(3)?).ok_or("unknown duration in trace")?,
            });
        }
        recorded.push(Recorded {
            step: num("step")? as u64,
            loc: j.get("loc").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            alternatives: alts,
            chosen: num("chosen")? as usize,
            notes,
        });
    }
    Ok(Trace { header, digest, recorded, last_step })
}
