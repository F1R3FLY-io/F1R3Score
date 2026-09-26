//! f1r3score: reads a textual score and plays a run of it.
//!
//! Exit codes: 0 ok, 1 usage, 2 parse or elaboration error, 3 runtime error,
//! 4 replay mismatch.

use score_chance::{Chance, ChoiceCtx, Choice};
use score_core::Q;
use score_engine::*;
use score_render as render;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::exit;

const USAGE: &str = "\
f1r3score -- the score-calculus player

usage:
  f1r3score check  SCORE
  f1r3score expand SCORE
  f1r3score play   SCORE [options]
  f1r3score replay TRACE --score SCORE [--midi FILE] [--bpm N] [--format table|json]
  f1r3score machine import FILE --kind pitch|dur [--dual] --score SCORE [--name NAME]

play options:
  --chance SPEC          default chance source: prng:SEED | spigot:C/B[@POS] | human   (default prng:0)
  --stream KEY=SPEC      bind a stream key (a label, a timbre, or KEY.pitch / KEY.dur)
  --scheduler S          canonical | random:SEED | by-timbre:A,B,...                   (default canonical)
  --mode max|one         fire a maximal matching (default) or one candidate (diagnostic)
  --gc off|freshness     collection of inert record locations                          (default off)
  --notes N  --until T  --max-steps N
  --max-alternatives N   (default 100000)   --unproductive N (default 100000)
  --max-states N         bound on behavioural window graphs (default 200000)
  --kmax K               interval-decoding digit bound (default 64)
  --trace FILE           JSONL trace (replayable)
  --format table|json    performance on stdout (default table)
  --midi FILE  --bpm N   Standard MIDI File (default 120 bpm)
  --live PORT            live MIDI output (built with --features live)
  --quiet                no performance on stdout
";

fn die(code: i32, msg: impl std::fmt::Display) -> ! {
    eprintln!("f1r3score: {msg}");
    exit(code)
}

struct Fs(PathBuf);
impl score_syntax::Loader for Fs {
    fn load(&mut self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| e.to_string())
    }
}

fn load(file: &str) -> score_syntax::Score {
    let src = std::fs::read_to_string(file).unwrap_or_else(|e| die(1, format!("{file}: {e}")));
    let dir = Path::new(file).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let s = score_syntax::load(file, &src, &mut Fs(dir)).unwrap_or_else(|e| die(2, e));
    for w in &s.warnings {
        eprintln!("{w}");
    }
    s
}

/// A human chooser: shows the alternatives and reads an index from stdin.
struct Human;
impl Chance for Human {
    fn choose(&mut self, weights: &[Q], ctx: &ChoiceCtx) -> Result<Choice, String> {
        let tot = weights.iter().fold(Q::ZERO, |a, w| a.add(w));
        let mut err = std::io::stderr();
        let _ = writeln!(err, "\nstep {} at onset {} (stream {}):", ctx.step, ctx.onset, ctx.stream);
        for (i, (l, w)) in ctx.labels.iter().zip(weights).enumerate() {
            let _ = writeln!(err, "  [{i}] {l}   ({:.3})", w.div(&tot).to_f64());
        }
        loop {
            let _ = write!(err, "choose 0..{}> ", weights.len() - 1);
            let _ = err.flush();
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("end of input while a choice was pending".into());
            }
            match line.trim().parse::<usize>() {
                Ok(i) if i < weights.len() => return Ok(Choice { index: i, digits: 0, fallback: false }),
                _ => {
                    let _ = writeln!(err, "  not an alternative");
                }
            }
        }
    }
    fn spec(&self) -> String {
        "human".into()
    }
}

fn source(spec: &str, kmax: u32) -> Box<dyn Chance> {
    if spec == "human" {
        return Box::new(Human);
    }
    score_chance::source(spec, kmax).unwrap_or_else(|e| die(1, e))
}

struct Opts {
    args: Vec<String>,
}
impl Opts {
    fn take(&mut self, flag: &str) -> Option<String> {
        let i = self.args.iter().position(|a| a == flag)?;
        if i + 1 >= self.args.len() {
            die(1, format!("{flag} needs a value"));
        }
        let v = self.args.remove(i + 1);
        self.args.remove(i);
        Some(v)
    }
    fn take_all(&mut self, flag: &str) -> Vec<String> {
        let mut v = vec![];
        while let Some(x) = self.take(flag) {
            v.push(x);
        }
        v
    }
    fn flag(&mut self, flag: &str) -> bool {
        match self.args.iter().position(|a| a == flag) {
            Some(i) => {
                self.args.remove(i);
                true
            }
            None => false,
        }
    }
    fn num<T: std::str::FromStr>(&mut self, flag: &str) -> Option<T> {
        self.take(flag).map(|v| v.parse().unwrap_or_else(|_| die(1, format!("bad value for {flag}: `{v}`"))))
    }
    fn done(&self) {
        if let Some(a) = self.args.first() {
            die(1, format!("unexpected argument `{a}`\n\n{USAGE}"));
        }
    }
}

struct Output {
    format: String,
    quiet: bool,
    midi: Option<String>,
    bpm: u32,
}

fn emit(e: &Engine, s_name: Option<&str>, digest: &str, o: &Output) {
    let perf = e.sorted_performance();
    if !o.quiet {
        if o.format == "json" {
            println!("{}", render::performance_json(&perf, &e.alph, s_name, digest));
        } else {
            print!("{}", render::performance_table(&perf, &e.alph));
        }
    }
    if let Some(m) = &o.midi {
        let midi = render::midi::write(&perf, &e.alph, o.bpm);
        if let Some(w) = &midi.warning {
            eprintln!("f1r3score: warning: {w}");
        }
        std::fs::write(m, &midi.bytes).unwrap_or_else(|e| die(3, format!("{m}: {e}")));
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        print!("{USAGE}");
        exit(if args.is_empty() { 1 } else { 0 });
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "check" => {
            let file = args.first().cloned().unwrap_or_else(|| die(1, USAGE));
            let s = load(&file);
            println!("ok {}", file);
            println!("digest  {}", s.digest_hex());
            println!("regime  {}", if s.behavioural { format!("window-behavioural (depth {})", s.window) } else { "local".into() });
            println!("terms   {}   clauses {}", s.arena.len(), s.clauses.len());
        }
        "expand" => {
            let file = args.first().cloned().unwrap_or_else(|| die(1, USAGE));
            let s = load(&file);
            print!("{}", score_syntax::print_score(&s));
        }
        "play" | "replay" => {
            let replaying = cmd == "replay";
            let first = args.first().cloned().unwrap_or_else(|| die(1, USAGE));
            args.remove(0);
            let mut o = Opts { args };
            let (score_file, trace_in) = if replaying {
                (o.take("--score").unwrap_or_else(|| die(1, "replay needs --score SCORE")), Some(first))
            } else {
                (first, None)
            };
            let s = load(&score_file);
            let digest = s.digest_hex();
            let name = s.name.clone();
            let mut cfg = Config::default();
            let mut sources_spec: Vec<String> = vec![];
            let mut chance = "prng:0".to_string();
            let mut trace = None;
            if let Some(t) = &trace_in {
                let text = std::fs::read_to_string(t).unwrap_or_else(|e| die(1, format!("{t}: {e}")));
                let tr = render::parse_trace(&text, &s.alph).unwrap_or_else(|e| die(4, e));
                if tr.digest != digest {
                    die(4, format!("digest mismatch: the trace is of score {}, this score is {}", tr.digest, digest));
                }
                let h = &tr.header;
                let hs = |k: &str| h.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                cfg.scheduler = Scheduler::parse(&hs("scheduler")).unwrap_or_else(|e| die(4, e));
                cfg.mode = if hs("mode") == "one" { Mode::One } else { Mode::Max };
                cfg.gc = hs("gc") == "freshness";
                trace = Some(tr);
            } else {
                chance = o.take("--chance").unwrap_or(chance);
                sources_spec = o.take_all("--stream");
                if let Some(x) = o.take("--scheduler") {
                    cfg.scheduler = Scheduler::parse(&x).unwrap_or_else(|e| die(1, e));
                }
                match o.take("--mode").as_deref() {
                    None | Some("max") => {}
                    Some("one") => cfg.mode = Mode::One,
                    Some(m) => die(1, format!("unknown mode `{m}`")),
                }
                match o.take("--gc").as_deref() {
                    None | Some("off") => {}
                    Some("freshness") => cfg.gc = true,
                    Some(g) => die(1, format!("unknown --gc `{g}`")),
                }
            }
            if let Some(x) = o.num("--max-alternatives") {
                cfg.max_alternatives = x;
            }
            if let Some(x) = o.num("--unproductive") {
                cfg.unproductive = x;
            }
            if let Some(x) = o.num("--max-states") {
                cfg.max_states = x;
            }
            if let Some(x) = o.num("--kmax") {
                cfg.kmax = x;
            }
            let mut limits = Limits {
                notes: o.num("--notes"),
                until: o.take("--until").map(|t| Q::parse(&t).unwrap_or_else(|| die(1, format!("bad time `{t}`")))),
                max_steps: o.num("--max-steps"),
            };
            if let Some(t) = &trace {
                // a replay reproduces exactly the recorded steps
                limits = Limits { notes: None, until: None, max_steps: Some(t.last_step.map_or(0, |s| s + 1)) };
            }
            if !replaying && limits.notes.is_none() && limits.until.is_none() && limits.max_steps.is_none() {
                eprintln!("f1r3score: note: no --notes/--until/--max-steps; an unbounded score will play forever");
            }
            let trace_out = o.take("--trace");
            let out = Output {
                format: o.take("--format").unwrap_or_else(|| "table".into()),
                quiet: o.flag("--quiet"),
                midi: o.take("--midi"),
                bpm: o.num("--bpm").unwrap_or(120),
            };
            let live = o.take("--live");
            o.done();
            let mut sources = Sources::new(source(&chance, cfg.kmax));
            for b in &sources_spec {
                let (k, v) = b.split_once('=').unwrap_or_else(|| die(1, format!("--stream wants KEY=SPEC, not `{b}`")));
                sources.bind(k, source(v, cfg.kmax));
            }
            let mut e = Engine::new(s.arena, s.clauses, s.alph, &s.initial, s.window, s.behavioural, cfg, sources)
                .unwrap_or_else(|err| die(3, err));
            let expected_steps = trace.as_ref().map(|t| t.recorded.clone());
            if let Some(rec) = expected_steps {
                e.set_replay(rec);
            }
            let mut tw = trace_out.map(|f| {
                let file = std::fs::File::create(&f).unwrap_or_else(|er| die(3, format!("{f}: {er}")));
                std::io::BufWriter::new(file)
            });
            if let Some(w) = tw.as_mut() {
                writeln!(w, "{}", render::trace_header(&e, &digest, &score_file)).ok();
            }
            #[cfg(feature = "live")]
            let mut live_out = live.map(|p| render::live::Live::open(&p, out.bpm).unwrap_or_else(|er| die(3, er)));
            #[cfg(not(feature = "live"))]
            if live.is_some() {
                die(1, "this build has no live MIDI; rebuild with `--features live`");
            }
            let mut last_step = None;
            let res = e.run(&limits, &mut |ev, eng| {
                if let Some(w) = tw.as_mut() {
                    writeln!(w, "{}", render::trace_event(ev, &eng.alph)).ok();
                }
                #[cfg(feature = "live")]
                if let Some(l) = live_out.as_mut() {
                    for n in &ev.notes {
                        if let Err(er) = l.play(n, &eng.alph) {
                            eprintln!("f1r3score: live MIDI: {er}");
                        }
                    }
                }
                last_step = Some(ev.step);
            });
            if let Some(mut w) = tw {
                w.flush().ok();
            }
            #[cfg(feature = "live")]
            if let Some(l) = live_out {
                l.finish().ok();
            }
            match res {
                Ok(stop) => {
                    if let Some(t) = &trace {
                        let consumed = t.recorded.iter().filter(|r| Some(r.step) <= last_step).count();
                        if consumed != t.recorded.len() {
                            die(4, format!("replay ended ({stop:?}) with {} recorded choices unused", t.recorded.len() - consumed));
                        }
                    }
                    emit(&e, name.as_deref(), &digest, &out);
                    eprintln!("f1r3score: {:?} after {} steps, {} notes", stop, e.steps, e.played);
                }
                Err(err) => {
                    emit(&e, name.as_deref(), &digest, &out);
                    die(if err.is_replay_mismatch() { 4 } else { 3 }, err)
                }
            }
        }
        "machine" => {
            if args.first().map(|s| s.as_str()) != Some("import") || args.len() < 2 {
                die(1, USAGE);
            }
            let file = args[1].clone();
            let mut o = Opts { args: args[2..].to_vec() };
            let kind = o.take("--kind").unwrap_or_else(|| die(1, "machine import needs --kind pitch|dur"));
            let dual = o.flag("--dual");
            let score = o.take("--score").unwrap_or_else(|| die(1, "machine import needs --score SCORE"));
            let fname = o.take("--name").unwrap_or_else(|| "imported".into());
            o.done();
            let s = load(&score);
            let src = std::fs::read_to_string(&file).unwrap_or_else(|e| die(1, format!("{file}: {e}")));
            let body = score_syntax::parser::parse_machine_file(&file, &src).unwrap_or_else(|e| die(2, e));
            let score_syntax::ast::MachineBody::Indexed { n, edges } = body else { unreachable!() };
            let states: Vec<score_core::Datum> = if kind == "pitch" {
                let mut v = s.alph.ordered_pitches().to_vec();
                v.push(s.alph.rest());
                v
            } else {
                s.alph.positive_durations().to_vec()
            };
            if states.len() != n {
                die(2, format!("the machine has {n} states; the score's {kind} alphabet has {}", states.len()));
            }
            let arrows: Vec<String> = edges
                .iter()
                .map(|(a, b, w)| format!("{} -> {} @ {w}", s.alph.name(states[*a]), s.alph.name(states[*b])))
                .collect();
            println!("factor {fname} = machine {kind}{} {{ {} }}", if dual { " dual" } else { "" }, arrows.join(", "));
        }
        #[cfg(feature = "live")]
        "ports" => {
            for p in render::live::ports().unwrap_or_else(|e| die(3, e)) {
                println!("{p}");
            }
        }
        _ => die(1, format!("unknown command `{cmd}`\n\n{USAGE}")),
    }
}
