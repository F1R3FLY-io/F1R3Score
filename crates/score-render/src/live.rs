//! Live MIDI output (feature `live`): events are sent as they are resolved,
//! scheduled in wall-clock time from their onsets at a given tempo.

use score_core::{Alphabets, Q};
use score_engine::Note;
use std::time::{Duration, Instant};

pub struct Live {
    conn: midir::MidiOutputConnection,
    start: Instant,
    bpm: u32,
    pending: Vec<(Q, [u8; 3])>,
}

pub fn ports() -> Result<Vec<String>, String> {
    let out = midir::MidiOutput::new("f1r3score").map_err(|e| e.to_string())?;
    Ok(out.ports().iter().filter_map(|p| out.port_name(p).ok()).collect())
}

impl Live {
    /// Connect to the first output port whose name contains `port`, ignoring
    /// case, and select each declared timbre's General MIDI program on its
    /// channel, as the MIDI file writer does.
    pub fn open(port: &str, bpm: u32, a: &Alphabets) -> Result<Live, String> {
        let out = midir::MidiOutput::new("f1r3score").map_err(|e| e.to_string())?;
        let ports = out.ports();
        let want = port.to_lowercase();
        let names: Vec<String> = ports.iter().map(|p| out.port_name(p).unwrap_or_default()).collect();
        let Some(i) = names.iter().position(|n| n.to_lowercase().contains(&want)) else {
            let listing = if names.is_empty() {
                "there are no MIDI output ports at all: start the synthesizer with a MIDI input enabled, \
                 then run `f1r3score ports`"
                    .to_string()
            } else {
                format!("the output ports are:\n  {}", names.join("\n  "))
            };
            return Err(format!("no MIDI output port matching `{port}`; {listing}"));
        };
        let mut conn = out.connect(&ports[i], "f1r3score").map_err(|e| e.to_string())?;
        for t in &a.timbres {
            if let Some(prog) = t.program {
                let ch = t.channel.saturating_sub(1) & 0x0f;
                conn.send(&[0xC0 | ch, prog & 0x7f]).map_err(|e| e.to_string())?;
            }
        }
        eprintln!("f1r3score: live MIDI to `{}`", names[i]);
        Ok(Live { conn, start: Instant::now(), bpm, pending: vec![] })
    }

    fn at(&self, t: &Q) -> Instant {
        let secs = t.to_f64() * 4.0 * 60.0 / self.bpm as f64;
        self.start + Duration::from_secs_f64(secs)
    }

    /// Queue a note; release everything due up to `now_onset`.
    /// Notes in the reserved dead and control timbres, and rests, are silent:
    /// they only advance the clock.
    pub fn play(&mut self, n: &Note, a: &Alphabets) -> Result<(), String> {
        let Some(t) = a.timbres.get(n.timbre.0 as usize) else {
            return self.flush(&n.onset);
        };
        let ch = t.channel.saturating_sub(1) & 0x0f;
        if let Some(key) = a.midi(n.pitch) {
            if a.len(n.dur).is_positive() {
                self.pending.push((n.onset.clone(), [0x90 | ch, key, 100]));
                self.pending.push((n.onset.add(&a.len(n.dur)), [0x80 | ch, key, 0]));
            }
        }
        self.flush(&n.onset)
    }

    pub fn flush(&mut self, upto: &Q) -> Result<(), String> {
        self.pending.sort_by(|a, b| (&a.0, a.1[0] & 0xf0).cmp(&(&b.0, b.1[0] & 0xf0)));
        while let Some(first) = self.pending.first() {
            if &first.0 > upto {
                break;
            }
            let (t, m) = self.pending.remove(0);
            let when = self.at(&t);
            let now = Instant::now();
            if when > now {
                std::thread::sleep(when - now);
            }
            self.conn.send(&m).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        let end = self.pending.iter().map(|x| x.0.clone()).max().unwrap_or(Q::ZERO);
        self.flush(&end)
    }
}
