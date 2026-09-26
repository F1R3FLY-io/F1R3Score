# f1r3score

A player for the score calculus of *Scores as Processes*
(`F1R3FLY-io/publications/f1r3score`), implementing revision 2 of
`f1r3score-player-spec.tex`. It reads a textual score and plays a run of it.
The run can be a table, JSON, a JSONL trace of records that replays exactly and
plays back without the score, a Standard MIDI File, or live MIDI.

```
cargo build --release
B=target/release/f1r3score

$B check  examples/twinkle.score          # parse, elaborate, digest, regime
$B expand examples/twinkle.score          # the elaborated core term
$B play   examples/twinkle.score --midi twinkle.mid
$B play   examples/four_hands.score --out perf.json   # two players, two keyboards
$B play   examples/chimera.score --midi chimera.mid   # the instrument chooses its timbre
$B play   examples/country_voice.score --notes 32 --stream A=prng:3 --stream B=prng:4 \
          --gc freshness --trace run.jsonl --midi run.mid
$B replay run.jsonl --score examples/country_voice.score
$B render run.jsonl --midi again.mid      # playback only: no score, no engine
$B play   examples/hocket.score --notes 16 --chance human    # choose each fitting yourself
```

To realise the instrument's π/e embedding as a score, route the voice's two
factors to two spigots (Lemma 6.4):

```
$B play crates/score-conformance/scores/t4_embedding.score --notes 16 --gc freshness \
   --stream A.pitch=spigot:pi/16@1 --stream A.dur=spigot:e/5
```

The resulting pitches are the hex digits of π, and the durations are the
base-5 digits of e.

Exit codes: 0 ok, 1 usage, 2 parse or elaboration error, 3 runtime error
(`unproductive`, `freshness-violated`, alternative limit), 4 replay mismatch.

## Writing for players and instruments

A key (a receipt holding a pitch) belongs to the instrument, and a touch (a
message holding a duration) belongs to the player. `std.score` provides
`Keyboard`, `Chimera`, `ChimeraKeyboard` and `touch`, and `;` gives
synchronous output: the player goes on when its note has ended.

```
def P1 = base "P1"
play Keyboard(P1, piano, 1, {A2, C3, E3, A3})
   | touch(P1, A3, q)!(0) ; touch(P1, C3, q)!(0) ; ( touch(P1, A3, h)!(0) | touch(P1, E3, h)!(0) ) ; 0
```

Names are `<@P, tau, d>`, with `_` for an open component. `@P` is the general
name `<@P, _, _>`, and `x!(Q)` passes `@Q`. In clause patterns, `?` matches
any component, and `_` matches only a wildcard.

## Layout

| crate | contents |
|---|---|
| `score-core` | exact rationals, alphabets, decorations with wildcards, hash-consed terms in congruence normal form, substitution, the instance order, matching, records and the playback function ν, bases and key locations, SHA-256 structural digests |
| `score-logic` | spatial / crisp / configuration / graded formulae, tables, the checkable-fragment type, candidate views built by ν, evaluation |
| `score-syntax` | lexer, parser, macro elaboration (including `;`), `std.score`, pretty-printer |
| `score-chance` | PRNG (SplitMix64 → xoshiro256\*\*), exact spigot digits, exact interval decoding |
| `score-engine` | location-only soup, matching, contention sets, maximal matchings, records, time, schedulers, freshness collection, factor routing, the behavioural explorer |
| `score-render` | playback from records, performance and trace JSON, replay parsing, MIDI writer, live MIDI (feature `live`) |
| `f1r3score` | the command-line player, including the human chooser and `render` |
| `score-conformance` | T0–T14, the additional tests, `gen_scores.py`, `differential.py`, `run_differential.sh` |

The five core crates perform no I/O (a test enforces this) and depend only on
`num-bigint` / `num-rational`.

## Tests

`cargo test --release` runs 70 tests. Among them:

- the ported suite T0–T12, on scores generated from `skeinsim.py`'s own random
  machines and seeds, extended as revision 2 asks: every request a voice
  delivers is a general name (T10), and every written note passes `@0` (T12);
- T13, the 4-hands piano: one performance under 20 random schedules, exactly
  the table of Example 9.5 with 16 null notes, and exactly the two
  performances the example describes when a third player joins;
- T14, the chimeric instrument: 4,500 open-timbre touches per condition,
  with the vibes share within 0.02 of 0.9, 0.5 and 0.1, and exactly 1 when the
  player names vibes;
- the instrument propositions (a key sounds once at a time, synchronous output
  sequences exactly, a chimera stays whole);
- records: every reported note is ν of its record, and `render` on a trace
  gives the same performance and MIDI file as `play`;
- matching unit tests, and the congruence and round-trip property tests over
  wildcards (10⁴ random pairs against an independent decision procedure);
- replay of every conformance run, and detection of tampered choices;
- fans over a subset against full fans with a crisp exclusion, record for
  record;
- collection: nothing is collected on an instrument, and the freshness guard;
- enforcement without deadlock, using Viable_β; chords; the Twinkle and
  FourHands MIDI golden files.

`run_differential.sh PUBLICATIONS/f1r3score` drives `skeinsim.py` with traces
from the player on T1, T5, T12, T13 (with and without the third player, six
schedules) and T14, and checks that both implementations emit the same
records at every resolution.

See `DESIGN.md` for decisions, deviations and what is and is not verified.
