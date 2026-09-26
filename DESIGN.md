# Design notes and status

These notes record, against `f1r3score-player-spec.tex`, what was built, what
was decided where the specification left room, where the implementation
deviates, and what remains unverified.

## Status by phase

| phase | status |
|---|---|
| P0 | Done. Arena, normal forms, substitution, digests; parser, elaborator, printer; round trip; congruence property test; Twinkle. |
| P1 | Done. Soup, candidates, contention, maximal matchings, time, three schedulers, performance, trace; T3, T6, T12. |
| P2 | Done. Atoms, spatial formulae, tables, machines, exact weights, PRNG with interval decoding, macros, `par`; T0, T1, T5, T7, T8, T9. |
| P3 | Done. `std.score` with the reflective voice, dual, hocket and handoff; freshness collection; stream keys; T2, T10, T11; freshness guard. |
| P4 | Done. MIDI writer, spigot sources, factor routing, replay, live MIDI; T4, replay, MIDI golden. Live MIDI compiles against ALSA but is untested on a device. |
| P5 | Done within the stated fragment. `leads`, `<g>`, `nu`, `live`; chord receipts; enforcement and chord tests. |
| P6 | Partial. The human chooser and the differential job are done. The wasm32 build is not verified: this build machine has no wasm32 std, so it is a CI job. |

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
| T9 | Tables equal their formulae on 3,000 random views. |
| T10 | The reflective `Voice` and an unrolled voice of 600 written steps give identical performances over 590 notes. Null notes are all (r, eps), two per note. |
| T11 | One record location with a candidate; 15 dead hands per note. |
| T12 | 22 notes, ending at 4, identical under every scheduler. |

Enforcement: with `Viable_β` the voice plays 10⁴ notes with no violation and
never falls silent; the window graph has 30 states. Without the factor, the
same score deadlocks on every seed tried.

Differential against `skeinsim.py`:
- T1 and T5: 1,999 fittings each agree on notes, carried data and number of
  alternatives.
- T12: 22 notes in both implementations.

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
     bodies and payloads, except bases.
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
7. **Factor routing (Lemma 5.3)** applies when three conditions hold:
   - `KEY.pitch` and `KEY.dur` are both bound;
   - the set is a star;
   - every top-level factor reads only the pitch-side or only the duration-side
     varying datum, and the candidates form the full product.
   Otherwise the draw is joint. The trace records which happened.
8. **`Responder` takes two clauses.** The first fitting happens at the
   rendezvous, which is a base. There `at`, `call[...]` and `prev` read nothing,
   so the first clause reads the call through `passes(...)`, exactly as
   Proposition 6.5 says; the server's clause reads it later through `at(...)`.
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
- **Differential scope.** The differential covers T1, T5 and T12. skeinsim's
  alternatives are in a different order from the player's canonical order, so
  the adapter identifies the recorded choice by what it emits (note and carried
  datum) rather than by index.

## Performance

A 16-pitch × 5-duration voice plays at about 0.5 ms per note in a release
build. T1 takes about 10 s for 20,000 notes, and the enforcement run about
5 s for 10⁴ notes.

Terms are hash-consed and never freed. Each note interns about 80 new fan
records, so memory grows linearly over very long runs.
