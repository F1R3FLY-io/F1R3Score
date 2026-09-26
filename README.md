# f1r3score

A player for the score calculus of *Scores as Processes*
(`F1R3FLY-io/publications/f1r3score`), implementing
`f1r3score-player-spec.tex`. It reads a textual score and plays a run of it.
The run can be a table, JSON, a JSONL trace that replays exactly, a Standard
MIDI File, or live MIDI.

```
cargo build --release
B=target/release/f1r3score

$B check  examples/twinkle.score          # parse, elaborate, digest, regime
$B expand examples/twinkle.score          # the elaborated core term
$B play   examples/twinkle.score --midi twinkle.mid
$B play   examples/country_voice.score --notes 32 --stream A=prng:3 --stream B=prng:4 \
          --gc freshness --trace run.jsonl --midi run.mid
$B replay run.jsonl --score examples/country_voice.score
$B play   examples/hocket.score --notes 16 --chance human    # choose each fitting yourself
```

To realise the instrument's π/e embedding as a score, route the voice's two
factors to two spigots (Lemma 5.3):

```
$B play crates/score-conformance/scores/t4_embedding.score --notes 16 --gc freshness \
   --stream A.pitch=spigot:pi/16@1 --stream A.dur=spigot:e/5
```

The resulting pitches are the hex digits of π, and the durations are the
base-5 digits of e.

Exit codes: 0 ok, 1 usage, 2 parse or elaboration error, 3 runtime error,
4 replay mismatch.

## Layout

| crate | contents |
|---|---|
| `score-core` | exact rationals, alphabets, hash-consed terms in congruence normal form, substitution, bases, SHA-256 structural digests |
| `score-logic` | spatial / crisp / configuration / graded formulae, tables, the checkable-fragment type, evaluation |
| `score-syntax` | lexer, parser, macro elaboration, `std.score`, pretty-printer |
| `score-chance` | PRNG (SplitMix64 → xoshiro256\*\*), exact spigot digits, exact interval decoding |
| `score-engine` | soup, candidates, contention sets, maximal matchings, time, schedulers, freshness collection, factor routing, the behavioural explorer |
| `score-render` | performance and trace JSON, replay parsing, MIDI writer, live MIDI (feature `live`) |
| `f1r3score` | the command-line player, including the human chooser |
| `score-conformance` | T0–T12, the additional tests, `gen_scores.py`, `differential.py` |

The five core crates perform no I/O (a test enforces this) and depend only on
`num-bigint` / `num-rational`.

## Tests

`cargo test --release` runs 48 tests. Among them:

- the ported suite T0–T12, on scores generated from `skeinsim.py`'s own random
  machines and seeds;
- the congruence property test (10⁴ random pairs against an independent
  decision procedure);
- replay of every conformance run, and detection of tampered choices;
- enforcement without deadlock, using Viable_β;
- chords, the freshness guard, and the Twinkle MIDI golden file.

`differential.py` drives `skeinsim.py` with a trace from the player and checks
that the two agree fitting by fitting.

See `DESIGN.md` for decisions, deviations and what is and is not verified.
