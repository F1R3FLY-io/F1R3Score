//! `score-render`: playback (`nu` on records), performance and trace
//! serialisation, replay parsing, a Standard MIDI File writer, and (feature
//! `live`) live MIDI output.
//!
//! Every note this crate prints, writes or sends is computed by applying the
//! playback function [`nu`] to a record (R-playback). Given a trace, playback
//! needs neither the score nor the engine: see [`render_trace`].

pub mod json;
pub mod midi;
#[cfg(feature = "live")]
pub mod live;

use json::J;
use score_core::{Alphabets, Datum, Deco, DurDecl, Hat, PitchDecl, ProcId, Timbre, TimbreDecl, Q};
use score_engine::{Engine, Event, Note, Recorded};

/// The playback function `nu: Record -> Note` (Definition 2.8), the same one
/// the engine uses to build candidate views.
pub use score_core::{nu, PlainNote, Record};

pub const TRACE_FORMAT: &str = "f1r3score-trace/2";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

/// The performance as JSON (spec, "Performance"): notes sorted by (onset,
/// timbre, pitch, duration), so that two runs with the same performance give
/// byte-identical files.
pub fn performance_json(notes: &[Note], a: &Alphabets, digest: &str) -> String {
    let end = notes.iter().map(|n| n.onset.add(&a.len(n.dur))).max().unwrap_or(Q::ZERO);
    let arr = notes
        .iter()
        .map(|n| {
            J::obj(vec![
                ("onset", J::s(n.onset.to_string())),
                ("timbre", J::s(a.timbre_name(n.timbre))),
                ("pitch", J::s(a.name(n.pitch))),
                ("dur", J::s(a.name(n.dur))),
                ("len", J::s(a.len(n.dur).to_string())),
            ])
        })
        .collect();
    J::obj(vec![("score", J::s(format!("sha256:{digest}"))), ("end", J::s(end.to_string())), ("notes", J::Arr(arr))])
        .to_string()
}

fn hat_t(a: &Alphabets, t: Hat<Timbre>) -> J {
    match t {
        Hat::Is(t) => J::s(a.timbre_name(t)),
        Hat::Wild => J::s("_"),
    }
}
fn hat_d(a: &Alphabets, d: Hat<Datum>) -> J {
    match d {
        Hat::Is(d) => J::s(a.name(d)),
        Hat::Wild => J::s("_"),
    }
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
        (
            "timbres",
            J::Arr(
                a.timbres
                    .iter()
                    .map(|t| J::Arr(vec![J::s(&t.name), t.program.map(|p| J::Num(p as i64)).unwrap_or(J::Null), J::Num(t.channel as i64)]))
                    .collect(),
            ),
        ),
        ("sources", J::Obj(e.sources.describe().into_iter().map(|(k, v)| (k, J::s(v))).collect())),
        ("scheduler", J::s(e.config.scheduler.spec())),
        ("mode", J::s(if e.config.mode == score_engine::Mode::Max { "max" } else { "one" })),
        ("regime", J::s(e.regime())),
        ("gc", J::s(if e.config.gc { "freshness" } else { "off" })),
        ("kmax", J::Num(e.config.kmax as i64)),
    ])
    .to_string()
}

/// One trace event per resolution: step, onset, location, set size,
/// alternatives, chosen index, routing, digits consumed per stream, and for
/// each fired candidate its record -- both subjects' decorations and the
/// payload's, with the location and payload processes by digest (R-trace).
/// Notes are not written: they are `nu` of the records.
pub fn trace_event(ev: &Event, a: &Alphabets) -> String {
    let recs = ev
        .records
        .iter()
        .zip(ev.record_keys.iter())
        .map(|((t, r), (lk, pk))| {
            J::obj(vec![
                ("onset", J::s(t.to_string())),
                ("loc", J::s(lk)),
                ("recv", J::Arr(vec![hat_t(a, r.recv.timbre), hat_d(a, r.recv.datum)])),
                ("send", J::Arr(vec![hat_t(a, r.send.timbre), hat_d(a, r.send.datum)])),
                ("payload", J::Arr(vec![J::s(pk), hat_t(a, r.ptimbre), hat_d(a, r.carry)])),
            ])
        })
        .collect();
    J::obj(vec![
        ("step", J::Num(ev.step as i64)),
        ("onset", J::s(ev.onset.to_string())),
        ("loc", J::s(&ev.loc)),
        ("set", J::Num(ev.set_size as i64)),
        ("alts", J::Num(ev.alternatives as i64)),
        ("chosen", J::Num(ev.chosen as i64)),
        ("routing", J::s(&ev.routing)),
        ("digits", J::Obj(ev.digits.iter().map(|(k, d)| (k.clone(), J::Num(*d as i64))).collect())),
        ("records", J::Arr(recs)),
    ])
    .to_string()
}

/// A record as read back from a trace: processes are known only by digest,
/// so they are replaced by a placeholder id; playback does not look at them.
fn parse_record(j: &J, a: &Alphabets) -> Result<(Q, Record), String> {
    let st = |x: Option<&J>| x.and_then(|v| v.as_str()).map(|s| s.to_string()).ok_or("bad record");
    let ht = |x: Option<&J>| -> Result<Hat<Timbre>, String> {
        let s = st(x)?;
        if s == "_" {
            return Ok(Hat::Wild);
        }
        a.lookup_any_timbre(&s).map(Hat::Is).ok_or(format!("unknown timbre `{s}` in trace"))
    };
    let hd = |x: Option<&J>| -> Result<Hat<Datum>, String> {
        let s = st(x)?;
        if s == "_" {
            return Ok(Hat::Wild);
        }
        a.lookup_datum(&s).map(Hat::Is).ok_or(format!("unknown datum `{s}` in trace"))
    };
    let arr = |k: &str| j.get(k).and_then(|v| v.as_arr()).ok_or(format!("record without `{k}`"));
    let (rv, sv, pv) = (arr("recv")?, arr("send")?, arr("payload")?);
    let onset = Q::parse(&st(j.get("onset"))?).ok_or("bad onset")?;
    Ok((
        onset,
        Record {
            loc: ProcId(0),
            recv: Deco { timbre: ht(rv.first())?, datum: hd(rv.get(1))? },
            send: Deco { timbre: ht(sv.first())?, datum: hd(sv.get(1))? },
            payload: ProcId(0),
            ptimbre: ht(pv.get(1))?,
            carry: hd(pv.get(2))?,
        },
    ))
}

/// The alphabets a trace header records (for playback without the score).
pub fn header_alphabets(h: &J) -> Result<Alphabets, String> {
    let arr = |k: &str| h.get(k).and_then(|v| v.as_arr()).ok_or(format!("trace header has no `{k}`"));
    let mut pitches = vec![];
    for p in arr("pitches")? {
        let v = p.as_arr().ok_or("bad pitch")?;
        pitches.push(PitchDecl {
            name: v.first().and_then(|x| x.as_str()).ok_or("bad pitch")?.to_string(),
            midi: v.get(1).and_then(|x| x.as_i64()).map(|m| m as u8),
        });
    }
    let mut durations = vec![];
    for d in arr("durations")? {
        let v = d.as_arr().ok_or("bad duration")?;
        let name = v.first().and_then(|x| x.as_str()).ok_or("bad duration")?.to_string();
        if name == "eps" {
            continue;
        }
        let len = Q::parse(v.get(1).and_then(|x| x.as_str()).ok_or("bad duration")?).ok_or("bad length")?;
        durations.push(DurDecl { name, len });
    }
    let mut timbres = vec![];
    for t in arr("timbres")? {
        let v = t.as_arr().ok_or("bad timbre")?;
        timbres.push(TimbreDecl {
            name: v.first().and_then(|x| x.as_str()).ok_or("bad timbre")?.to_string(),
            program: v.get(1).and_then(|x| x.as_i64()).map(|p| p as u8),
            channel: v.get(2).and_then(|x| x.as_i64()).unwrap_or(1) as u8,
        });
    }
    Alphabets::new(pitches, durations, timbres).map_err(|e| e.0)
}

/// Playback of a trace: every record's note, by `nu`, with its onset. Needs
/// neither the score nor the engine. Returns the alphabets, the digest, and
/// all notes (null notes included).
pub fn render_trace(text: &str) -> Result<(Alphabets, String, Vec<Note>), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = json::parse(lines.next().ok_or("empty trace")?)?;
    if header.get("format").and_then(|x| x.as_str()) != Some(TRACE_FORMAT) {
        return Err(format!("not a {TRACE_FORMAT} trace"));
    }
    let a = header_alphabets(&header)?;
    let digest = header.get("digest").and_then(|x| x.as_str()).ok_or("trace header has no digest")?.to_string();
    let mut notes = vec![];
    for (i, l) in lines.enumerate() {
        let j = json::parse(l).map_err(|e| format!("trace line {}: {e}", i + 2))?;
        for r in j.get("records").and_then(|x| x.as_arr()).unwrap_or(&[]) {
            let (t, rec) = parse_record(r, &a).map_err(|e| format!("trace line {}: {e}", i + 2))?;
            let n = nu(&rec, &a).ok_or(format!("trace line {}: a record with no note", i + 2))?;
            notes.push(Note { onset: t, timbre: n.timbre, pitch: n.pitch, dur: n.dur });
        }
    }
    Ok((a, digest, notes))
}

/// The performance of a list of notes: those of positive length, sorted.
pub fn performance_of(notes: &[Note], a: &Alphabets) -> Vec<Note> {
    let mut v: Vec<Note> = notes.iter().filter(|n| a.len(n.dur).is_positive()).cloned().collect();
    v.sort_by(|x, y| (&x.onset, x.timbre, x.pitch, x.dur).cmp(&(&y.onset, y.timbre, y.pitch, y.dur)));
    v
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
        // the notes a replayed choice must emit: nu of the recorded records
        let mut notes = vec![];
        for r in j.get("records").and_then(|x| x.as_arr()).unwrap_or(&[]) {
            let (t, rec) = parse_record(r, a).map_err(|e| format!("trace line {}: {e}", i + 2))?;
            let n = nu(&rec, a).ok_or(format!("trace line {}: a record with no note", i + 2))?;
            notes.push(Note { onset: t, timbre: n.timbre, pitch: n.pitch, dur: n.dur });
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
