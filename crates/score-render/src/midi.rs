//! Standard MIDI File writer: format 1, one track per timbre.

use score_core::{Alphabets, Q};
use score_engine::Note;
use num_integer::Integer;

pub struct Midi {
    pub bytes: Vec<u8>,
    pub division: u16,
    pub warning: Option<String>,
}

fn vlq(mut n: u32, out: &mut Vec<u8>) {
    let mut buf = vec![(n & 0x7f) as u8];
    n >>= 7;
    while n > 0 {
        buf.push(((n & 0x7f) as u8) | 0x80);
        n >>= 7;
    }
    buf.reverse();
    out.extend(buf);
}

fn chunk(tag: &[u8; 4], body: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
}

/// The least ticks-per-quarter making every onset and length integral, or 960
/// (with rounding and a warning) above the format's 32767.
pub fn division(notes: &[Note], alph: &Alphabets) -> (u64, bool) {
    let mut l: u64 = 1;
    let q4 = Q::int(4);
    for n in notes.iter().filter(|n| !n.timbre.is_reserved()) {
        for x in [n.onset.mul(&q4), alph.len(n.dur).mul(&q4)] {
            let d = match &x {
                Q::S(_, d) => *d,
                Q::B(_) => return (960, true),
            };
            l = l.lcm(&d);
            if l > 32767 {
                return (960, true);
            }
        }
    }
    (l, false)
}

fn ticks(x: &Q, tpq: u64) -> u32 {
    let t = x.mul(&Q::int(4 * tpq));
    match t {
        Q::S(n, d) => ((n + d / 2) / d) as u32,
        Q::B(b) => {
            use num_traits::ToPrimitive;
            b.round().to_integer().to_u32().unwrap_or(u32::MAX)
        }
    }
}

pub fn write(notes: &[Note], alph: &Alphabets, bpm: u32) -> Midi {
    let (tpq, rounded) = division(notes, alph);
    let mut out = vec![];
    let ntracks = alph.timbres.len() + 1;
    let mut hdr = vec![0, 1];
    hdr.extend_from_slice(&(ntracks as u16).to_be_bytes());
    hdr.extend_from_slice(&(tpq as u16).to_be_bytes());
    chunk(b"MThd", &hdr, &mut out);
    // conductor track: tempo
    let mut t0 = vec![];
    let us = 60_000_000 / bpm.max(1);
    t0.extend_from_slice(&[0, 0xFF, 0x51, 3]);
    t0.extend_from_slice(&us.to_be_bytes()[1..]);
    t0.extend_from_slice(&[0, 0xFF, 0x2F, 0]);
    chunk(b"MTrk", &t0, &mut out);
    for (ti, t) in alph.timbres.iter().enumerate() {
        let ch = (t.channel.saturating_sub(1)) & 0x0f;
        let mut body = vec![0, 0xFF, 0x03];
        vlq(t.name.len() as u32, &mut body);
        body.extend_from_slice(t.name.as_bytes());
        if let Some(p) = t.program {
            body.extend_from_slice(&[0, 0xC0 | ch, p]);
        }
        // (tick, off-before-on, key, bytes)
        let mut evs: Vec<(u32, u8, u8, [u8; 3])> = vec![];
        // a note's track is chosen by its timbre -- the meet of the two
        // subjects' timbres -- so a chimeric key's notes go to whichever
        // timbre won; the reserved dead and control timbres have no track
        for n in notes.iter().filter(|n| n.timbre.0 as usize == ti) {
            let Some(key) = alph.midi(n.pitch) else { continue }; // rests are silence
            let len = alph.len(n.dur);
            if !len.is_positive() {
                continue;
            }
            let on = ticks(&n.onset, tpq);
            let off = ticks(&n.onset.add(&len), tpq);
            evs.push((on, 1, key, [0x90 | ch, key, 100]));
            evs.push((off.max(on), 0, key, [0x80 | ch, key, 0]));
        }
        evs.sort();
        let mut last = 0;
        for (t, _, _, b) in evs {
            vlq(t - last, &mut body);
            body.extend_from_slice(&b);
            last = t;
        }
        body.extend_from_slice(&[0, 0xFF, 0x2F, 0]);
        chunk(b"MTrk", &body, &mut out);
    }
    Midi {
        bytes: out,
        division: tpq as u16,
        warning: rounded.then(|| "onsets need more than 32767 ticks per quarter; rounded to 960".into()),
    }
}
