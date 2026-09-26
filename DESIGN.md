# Design notes and status

These notes record, against `f1r3score-player-spec.tex`, what was built, what
was decided where the specification left room, where the implementation
deviates, and what remains unverified.

## Status by phase

Phases follow revision 2 of the specification (26 September 2026).

| phase | status |
|---|---|
| P0 | Done. Arena with wildcard decorations, normal forms, matching, the instance order, substitution, digests; parser, elaborator, printer; round trip; congruence and round-trip property tests; matching unit tests; Twinkle. |
| P1 | Done. Location-only soup, candidates by matching, contention, maximal matchings, time, three schedulers; records and playback; performance; trace; `render`; T3, T6, T12; records test. |
| P2 | Done. Atoms (including `carry(_)`), spatial formulae with `?` and `_`, tables (including `dur`), machines, exact weights, PRNG with interval decoding, macros and `par` comprehensions; T0, T1, T5, T7, T8, T9. |
| P3 | Done. `std.score` voices with general-name requests, handoff with an open-timbre responder, cross-timbre hocket; stream labels; freshness collection restricted to record-shaped locations; T2, T10, T11; collection tests. |
| P4 | Done. Keys, keyboards, chimeras, `touch`; synchronous output `;` with a fresh acknowledgement location per site; T13, T14; instrument tests. |
| P5 | Done. MIDI writer (per-note timbre routing), spigot sources, factor routing, replay, live MIDI; T4, replay, MIDI goldens. Live MIDI compiles against ALSA but is untested on a device. |
| P6 | Done within the stated fragment. `leads`, `<g>`, `nu`, `live`; chord receipts; enforcement and chord tests. |
| P7 | Partial. The human chooser and the differential are done. The wasm32 build is not verified: this build machine has no wasm32 std, so it is a CI job. |

## Conformance results (release build)

| test | result |
|---|---|
| T0 | One record location with a candidate at every step; exactly 79 stale messages per note over 400 notes. |
| T1 | 20,000 notes. Pitch \|z\| ≤ 2.49, duration \|z\| ≤ 1.97, joint (independence) \|z\| ≤ 3.03. No off-machine transitions. |
| T2 | The same performance under canonical, three random and by-timbre schedules. More than 50 simultaneous strikes. |
| T3 | Max mode: 3 notes in the first resolution, 3 in total. One mode: 1, then 3. |
| T4 | Pitches are the hex digits of π and durations the base-5 digits of e, with one digit per draw ("factored" routing). |
| T5 | The dual's law has \|z\| ≤ 3.5. |
| T6 | Two notes per resolution; equal durations never occur (0 of 2,000). |
| T7 | Style × physicality matches the normalised product with \|z\| ≤ 3.5; the stepwise instrument steps more than the leaping one. |
| T8 | No triple repeats under the crisp rule; the motif completion rate rises by more than 0.2 under encouragement. |
| T9 | Tables equal their formulae on 3,000 random views, including a `dur`-keyed table and views whose carried datum is open. |
| T10 | The reflective `Voice` and an unrolled voice of 600 written steps give identical performances over 590 notes. Null notes are all (r, eps), two per note, and every request and refresh passes a general name. |
| T11 | One record location with a candidate; 15 dead keys per note. |
| T12 | 22 notes, ending at 4, identical under every scheduler; every written note passes `@0`. |
| T13 | Two players: exactly the table of Example 9.5 with 16 null notes, under 20 random schedules. With the third player: exactly the two performances the example describes, 18 null notes. |
| T14 | 4,500 open-timbre touches per condition. Vibes share 0.897 (eighths), 0.494 (quarters), 0.094 (halves); 1.000 when the player names vibes. No key location ever holds more than two receipts, and each holds one receipt per timbre whenever nothing there is sounding. |

Enforcement: with `Viable_β` the voice plays 10⁴ notes with no violation and
never falls silent; the window graph has 30 states. Without the factor, the
same score deadlocks on every seed tried.

Differential against `skeinsim.py` (revision of 26 September, 853 lines),
`run_differential.sh`:
- T1 and T5: 2,000 fittings each agree on notes, carried data, onsets and
  numbers of alternatives.
- T12: 22 notes in both implementations.
- T13: all 24 resolutions agree, null notes included. T13 with the third
  player: all 27 agree, under each of six schedules.
- T14 (90 touches): all 270 resolutions agree, including the timbre every note
  sounded in.

## Decisions taken where the spec left room

1. **Bases in the core syntax.** `base "tag"` elaborates to
   `for(_ <- <@L(tag), τ⊥, r>) 0`.
   - `L(tag)` encodes the tag's bytes as numerals and lists built only from
     messages on τ⊥.
   - The elaborated term is therefore a closed core term and nothing else, and
     distinct tags give inequivalent bases.
   - Receipts on τ⊥ are never brought to top level.
2. **Binders.** Bound names are de Bruijn indices in terms. While a binder is
   open, the elaborator uses de Bruijn levels and closes them into indices.
   - Macro expansion is memoised on (definition, argument values). Its result
     does not depend on the binder depth.
   - This keeps an unrolled voice linear in size rather than exponential: every
     hand's continuation is the same next step.
3. **Stream labels.**
   - `#A` on an application labels every receipt in its expansion, through
     bodies and payloads, except receipts on the dead timbre.
   - Labels are excluded from digests.
   - A continuation inherits the stream of the receipt that fired it.
   - Unbound keys share one instance of the default source.
4. **The initial configuration** is one parallel composition per distinct stamp.
5. **Replay** is keyed by step, location key, number of alternatives and
   chosen index, and the notes emitted are checked. The scheduler, mode and
   collection setting come from the trace header, and chance sources are
   ignored. A run that diverges exits with 4.
6. **Driver positions.** `spigot:C/B@Q` starts a spigot at digit Q, the driver
   position of the drawn-distributions design. T4 needs `@1` because the
   voice's first pitch is written in the score rather than drawn.
7. **Factor routing (Lemma 6.4)** applies when three conditions hold:
   - `KEY.pitch` and `KEY.dur` are both bound;
   - the set is a star;
   - every top-level factor reads only the pitch-side or only the duration-side
     varying datum, and the candidates form the full product.
   Otherwise the draw is joint. The trace records which happened.
8. **`Responder` takes two clauses.** The first fitting happens at the
   rendezvous, which is a base. There `at`, `call[...]` and `prev` read nothing,
   so the first clause reads the call through `passes(...)`, exactly as
   Proposition 7.5 says; the server's clause reads it later through `at(...)`.
9. **Clause comprehensions.** `any/all x in S { β }` and `sum x in S { ψ }`
   expand at elaboration. They are needed to write "no triple repeat" (T8)
   without a special atom. `call[...]` skips the outermost record, which was
   the spec's proposal for decision 8.
10. **Spec decisions to confirm** all follow the proposals:
    - a new repository;
    - the CFL dialect, with names written `<@P, tau, d>` and the extension `.score`;
    - exact rationals only;
    - canonical scheduling by default;
    - the longest-duration chord convention;
    - no automatic skipping in `last[...]`;
    - scientific pitch names, with enharmonics kept as distinct data.

## Revision 2: decisions taken where the spec left room

1. **Wildcards are a type.**
   - Decorations are `Hat<T> = Is(T) | Wild` throughout, and `Hat::Wild` equals
     only `Hat::Wild`.
   - `meet` is used by matching and playback, and by nothing else.
   - The instance order `x ⊑ x'` is `score_core::instance_of`. Only the
     round-trip property test uses it, because matching needs only the meet.
2. **Records and ν live in `score-core`.**
   - The engine builds candidate views with `score_core::nu`, and
     `score-render` re-exports the same function.
   - An `Event` carries the records it fired, and its notes are computed from
     them.
   - The trace (format `f1r3score-trace/2`) stores records and no notes.
3. **Chord receipts produce one record per pattern.** Each record pairs that
   pattern's subject with the message it matched. The continuation's start
   time is the single function `chord_release` (open thread 5).
4. **Reserved timbres.**
   - `dead` (τ⊥) cannot be written. Only `base`, `keyloc` and `keycode` emit
     it.
   - `ctl` (κ) can be written, so that `expand` output containing
     acknowledgements re-parses.
   - Neither `dead`, `ctl` nor `_` can be declared.
5. **Key, code and acknowledgement locations** are built in `score_core::base`:
   - `keyloc(P, k) = <@P, dead, k>!(0)`;
   - `keycode(P, k, tau) = <@P, dead, k>!(0, tau, _)`;
   - `ackloc(n) = <@L(n), ctl, eps>!(0, ctl, eps)`.

   None of these is record-shaped. The printer shows them as the builtins
   `keyloc(...)`, `keycode(...)` and `ackloc(n)`, so printed terms re-parse.
   `ackloc(n)` is the printed form of an acknowledgement location; it is not
   meant to be written.
6. **Freshness of acknowledgement locations** (decision 4 of the spec).
   - Each `;` the elaborator meets gets a new `ackloc(n)`.
   - A `;` inside a message payload is an elaboration error. This is the
     conservative reading of "under a quote that can be re-run", because a
     payload is a quote.
   - Expansions are memoised on (definition, arguments, inside-a-payload).
     Two identical, memoised expansions share their acknowledgement location,
     since they are the same term.
7. **Stream labels.**
   - An unlabelled receipt that inherits nothing takes its subject timbre, or
     `loc:<digest>` when the timbre is open (Decision "stream labels").
   - A chimera's contention set draws from the stream of its least receipt
     occurrence, the first-installed timbre.
8. **Surface syntax additions.**
   - Parameter sorts `pitchset`, `[S]` and `(S1, S2, ...)`.
   - Top-level `pitchset NAME = {...}`.
   - `par (a, b) in list { ... }`.
   - Definitions without parameters: `def P1 = base "P1"`.
   - Definitions returning a name: `def touch(...): name = ...`.
   - `piano88`.
   - Inside definition arguments, `;` separates arguments, as before. A
     sequence written there must be parenthesised.
9. **The library.**
   - `Fan` takes a pitch set, and `StepOver` and `VoiceOver` use it.
     `Step`, `Voice` and `Server` keep their revision-1 signatures over all
     pitches.
   - `Hocket` takes two timbres, one per player.
   - `RespAny` and `ResponderAny` are the open-timbre responder of §7.4.
   - `ChimeraKeyboard` installs a chimera on every key of a pitch set.
10. **Clause patterns.**
    - A location is never a wildcard, so `<_, ...>` is rejected with a pointer
      to `?`.
    - `pitch(_)` and `dur(_)` are rejected, because the note's data are always
      concrete.
11. **Lints.**
    - A warning for a subject with datum `_`.
    - A warning for a location where no receipt and message match. It says so
      specifically when every timbre there is open.
    - Revision 1's payload-timbre warning is gone.

## Revision 2: findings

- **The two tie-breaks of the differential.** When several of `skeinsim`'s
  alternatives emit exactly the recorded records, they differ only in the
  payload process. An example is two players' touches `A3 q` at one key. The
  two implementations represent processes incomparably, so the adapter
  searches over those ties. It accepts a trace only if some resolution of the
  ties reproduces every resolution.
- **Collection with instruments.** The spec's Finding on `skeinsim`'s
  collection test holds here.
  - The engine collects only record-shaped locations, and so does the
    behavioural explorer's pruning, which had used the old test.
  - FourHands with `--gc freshness` collects nothing and plays identically.

## The window-behavioural fragment

`leads(φ)` holds of a candidate when the residue of firing it, in the present
configuration, satisfies φ. Here φ ranges over `true`, `false`, `live`, `!`,
`&&`, `||`, `<g> φ`, `<> φ`, `nu X. φ` and `X`, and the guard `g` is a local
crisp formula.

- **Decision method.** φ is decided on the finite graph of configurations
  reachable by single communications.
  - Stamps are erased.
  - Record locations with no locally available candidate are collected.
  - Configurations are identified by their contents, with every quote truncated
    to the score's window, which is the greatest spatial depth any clause reads.
  - `nu` is computed by iteration from the top.
- **Graph and caching.** The graph and each formula's value are cached across
  the run. A viable voice's graph is built once and then only looked up.
- **Availability inside exploration** is the clause with its behavioural
  factors dropped, its local part.
- **Soundness assumptions**, both enforced:
  - freshness: a spawn at a collected location is `freshness-violated`;
  - a state bound (`--max-states`): exceeding it is an error, never a
    truncation.
- **Not admitted:** graded modalities (`<>ψ` valued in R≥0) and graded fixed
  points. The parser rejects them with a reason.

## `spigot_stream` defects found (F1R3Games HEAD, commit ed61943)

The spec names `spigot_stream` as the digit source. At the time of writing it
could not serve outside base 10:

- `PiStream::with_base(16)` yields `0, 5` and then `None`. `with_base(2)`
  yields `0, 1, 0` and then `None`. The first digit is 0, not the 3 its own
  test asserts.
- `Ln2Stream::with_base(10)` panics on u64 overflow (lib.rs:341).
- `EStream` omits the integer digit, contrary to its documentation and its
  `e_base10_first_15` test.

`score-chance` therefore computes digits itself:
- π, e and ln 2 come from integer series with explicit error bounds, and a
  digit is emitted only when both bounds agree on it.
- Liouville, Champernowne and Thue–Morse come from their positional rules.

The digits are checked against known expansions, including 128 hex digits of
π. Once the crate is fixed, the feature can switch back.

## Not verified here

- **wasm32 build.** No wasm32 std is available on this machine. The five core
  crates contain no I/O (enforced by a test), and CI builds them for
  `wasm32-unknown-unknown`.
- **Live MIDI** compiles against ALSA but has not been tested on a MIDI device.
- **Differential scope.** The differential covers T1, T5, T12, T13 and T14,
  not T0, T2–T4 and T6–T11 or random small scores, as the spec asks.
  `skeinsim`'s alternatives are ordered differently from the player's
  canonical order, so the adapter identifies each recorded choice by the
  records it emits, not by its index.

## Performance

A 16-pitch × 5-duration voice plays at about 0.5 ms per note in a release
build. T1 takes about 10 s for 20,000 notes, and the enforcement run about
5 s for 10⁴ notes.

Terms are hash-consed and never freed. Each note interns about 80 new fan
records, so memory grows linearly over very long runs.
