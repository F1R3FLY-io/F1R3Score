#!/usr/bin/env python3
"""Cross-implementation differential: drive skeinsim.py with the choices the
Rust player recorded in a trace, and check that both emit the same notes with
the same number of alternatives at every musical fitting.

    python3 differential.py PUBLICATIONS/f1r3score TRACE.jsonl SCENARIO

SCENARIO is one of t1, t5, t12.  skeinsim's alternatives are ordered
differently from the player's canonical order, so the adapter identifies the
recorded choice by what it emits (note and carried datum) rather than by index.
"""
import json, random, sys
sys.path.insert(0, sys.argv[1])
import skeinsim as S

trace = [json.loads(l) for l in open(sys.argv[2]) if l.strip()]
scen = sys.argv[3]
head, events = trace[0], trace[1:]
P_NAMES = [p[0] for p in head['pitches']]
D_NAMES = [d[0] for d in head['durations']]
musical = [e for e in events if any(D_NAMES.index(n[3]) < 5 for n in e['notes'])]

def note_of(n):
    return (S.P(P_NAMES.index(n[2])), S.D(D_NAMES.index(n[3])))

def forced_resolve(soup, want, want_carry, n_alts):
    """One skeinsim resolution forced to the recorded alternative."""
    comps = []
    for loc in soup.live_locations():
        for c in soup.components(loc):
            t = min(max(x[0]['stamp'], x[1]['stamp']) for x in c)
            comps.append((t, loc, c))
    tmin = min(t for t, _, _ in comps)
    due = [(loc, c) for t, loc, c in comps if t == tmin]
    assert len(due) == 1, 'one due set at a time in these scenarios'
    loc, comp = due[0]
    opts = S.maximal_matchings(comp)
    assert len(opts) == n_alts, f'alternatives: skeinsim {len(opts)}, player {n_alts}'
    pick = [o for o in opts if [c[3] for c in o] == want and
            (want_carry is None or o[0][1]['carry'] == want_carry)]
    assert len(pick) == 1, f'recorded choice {want} carry {want_carry} not unique in skeinsim'
    for (r, m, w, note) in pick[0]:
        soup.remove(loc, r, m)
        z = (m['payload'], loc[1], m['carry'])
        r['cont'](soup, z, max(r['stamp'], m['stamp']) + S.dur_value(note[1]))
    if S.node(loc[0])[0] == 'send' and not soup.candidates(loc):
        soup.M.pop(loc, None); soup.R.pop(loc, None)
    return [c[3] for c in pick[0]]

if scen in ('t1', 't5'):
    dual = scen == 't5'
    rng = random.Random(7)
    EP, ED = S.random_machine(16, rng), S.random_machine(5, rng, (2, 3))
    psi = S.dual_clause(EP, ED) if dual else S.machine_clause(EP, ED)
    soup = S.Soup()
    S.voice(soup, S.seed('A', 'piano', 0, 2, dual=dual), 0, psi, 16, 5, 'A', dual=dual)
    notes = [note_of(e['notes'][0]) for e in musical]
    for i, e in enumerate(musical[:-1]):
        # pitch leads: the carry is the next note's pitch; rhythm leads: its duration
        carry = notes[i + 1][1] if dual else notes[i + 1][0]
        got = forced_resolve(soup, [notes[i]], carry, e['alts'])
        assert got == [notes[i]]
    print(f'{scen}: {len(musical) - 1} fittings agree (notes, carried data, alternative counts)')
elif scen == 't12':
    # the written tune: every fitting has one alternative; compare the tables
    print('t12:', 'player notes', sum(len(e['notes']) for e in events),
          '; skeinsim', S.T12_twinkle())
    assert sum(len(e['notes']) for e in events) == S.T12_twinkle()['notes']
else:
    sys.exit('unknown scenario')
