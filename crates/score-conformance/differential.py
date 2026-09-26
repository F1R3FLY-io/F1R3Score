#!/usr/bin/env python3
"""Cross-implementation differential (spec, "Additional tests"): drive
skeinsim.py with the choices the Rust player recorded in a trace, and check
that both implementations emit the same records -- the same notes, carried
data and payload decorations, with the same number of alternatives -- at every
resolution.

    python3 differential.py PUBLICATIONS/f1r3score TRACE.jsonl SCENARIO

SCENARIO is one of t1, t5, t12, t13, t13third, t14.  The trace must come from
the matching score in scores/ (t1_law, t5_dual, t12_twinkle, t13_fourhands,
t13_third, t14_chimera_small).

skeinsim orders alternatives, and simultaneously due sets, differently from
the player's canonical order, so the adapter does not replay indices: for each
recorded event it finds the due set in skeinsim that has an alternative
emitting exactly the recorded records, checks that the numbers of
alternatives agree, and fires that alternative.  For t1 and t5 skeinsim's
voice writes its recursion in the metalanguage (no null notes), so only the
player's musical events are compared (Finding 10.3 is that the two agree).
"""
import json, math, random, sys
from fractions import Fraction
sys.path.insert(0, sys.argv[1])
import skeinsim as S

trace = [json.loads(l) for l in open(sys.argv[2]) if l.strip()]
scen = sys.argv[3]
head, events = trace[0], trace[1:]
P_NAMES = [p[0] for p in head['pitches']]
D_NAMES = [d[0] for d in head['durations']]
POS_D = [d[0] for d in head['durations'] if Fraction(d[1]) > 0]


def rec_player(r):
    """(timbre, pitch, dur, carry, payload decoration) of a player record."""
    (t1, d1), (t2, d2) = r['recv'], r['send']
    tau = t1 if t1 != '_' else t2
    pitch, dur = (d1, d2) if d1 in P_NAMES else (d2, d1)
    _, pt, carry = r['payload']
    return (tau, pitch, dur, carry, pt)


def dname(d, dual=False):
    """A skeinsim datum as the player names it."""
    if d == S.WILD: return '_'
    if d == S.NULL: return 'eps'
    if d == S.REST: return 'r'
    kind, v = d
    if kind == 'p': return P_NAMES[v] if isinstance(v, int) else v
    if isinstance(v, int): return POS_D[v]
    return v[0]


def rec_skein(loc, r, m):
    """The same five fields of a skeinsim candidate, read off its record."""
    rec = S.record(loc, r, m)
    tau, pitch, dur = S.note_of(rec)
    _, pt, carry = rec['payload']
    return (tau, dname(pitch), dname(dur), dname(carry) if carry != S.WILD else '_', pt if pt == S.WILD else pt)


def key_of(x, with_carry):
    tau, p, d, c, pt = x
    return (tau, p, d) + ((c,) if with_carry else ())


class Tie(Exception):
    """More than one alternative emits the recorded records."""


def drive(soup, evs, with_carry=True, picks=None, ties=None, ticks=1):
    """Force skeinsim through the recorded events. Where several of its
    alternatives emit exactly the recorded records (they differ only in the
    payload process, which the two implementations represent incomparably),
    `picks[step]` chooses among them; the tied steps are appended to `ties`."""
    picks = picks or {}
    n = 0
    for e in evs:
        want = sorted(key_of(rec_player(r), with_carry) for r in e['records'])
        comps = []
        for loc in soup.live_locations():
            for c in soup.components(loc):
                t = min(max(x[0]['stamp'], x[1]['stamp']) for x in c)
                comps.append((t, loc, c))
        assert comps, f'step {e["step"]}: skeinsim has nothing left to play'
        tmin = min(t for t, _, _ in comps)
        # skeinsim's voices keep time in ticks (16 per whole note)
        assert Fraction(tmin) / ticks == Fraction(e['onset']), \
            f'step {e["step"]}: onset: skeinsim {tmin}, player {e["onset"]}'
        cands = []
        for t, loc, comp in comps:
            if t != tmin: continue
            opts = S.maximal_matchings(comp)
            for S_ in opts:
                got = sorted(key_of(rec_skein(loc, c[0], c[1]), with_carry) for c in S_)
                if got == want:
                    cands.append((loc, comp, S_, len(opts)))
        assert cands, f'step {e["step"]}: no due set in skeinsim emits {want}'
        if len(cands) > 1 and ties is not None:
            ties.append((e['step'], len(cands)))
        loc, comp, S_, nalts = cands[picks.get(e['step'], 0) % len(cands)]
        assert nalts == e['alts'], f'step {e["step"]}: alternatives: skeinsim {nalts}, player {e["alts"]}'
        for (r, m, w, note) in S_:
            soup.remove(loc, r, m)
            onset = max(r['stamp'], m['stamp'])
            z = (m['payload'], m['ptau'], m['carry'])
            r['cont'](soup, z, onset + S.dur_value(note[1]))
        if S.node(loc[0])[0] == 'send' and not soup.candidates(loc) and scen in ('t1', 't5'):
            soup.M.pop(loc, None); soup.R.pop(loc, None)
        n += 1
    return n


def musical(e):
    return any(rec_player(r)[2] in POS_D for r in e['records'])


if scen in ('t1', 't5'):
    dual = scen == 't5'
    rng = random.Random(7)
    EP, ED = S.random_machine(16, rng), S.random_machine(5, rng, (2, 3))
    psi = S.dual_clause(EP, ED) if dual else S.machine_clause(EP, ED)
    soup = S.Soup()
    S.voice(soup, S.seed('A', 'piano', 0, 2, dual=dual), 0, psi, 16, 5, 'A', dual=dual)
    evs = [e for e in events if musical(e)]
    n = drive(soup, evs, ticks=16)
    print(f'{scen}: {n} fittings agree (notes, carried data, onsets, alternative counts)')
elif scen == 't12':
    # the written tune: every fitting has one alternative
    evs = [e for e in events]
    assert all(e['alts'] == 1 for e in evs)
    got = S.T12_twinkle()
    player = sum(len(e['records']) for e in evs)
    assert player == got['notes'], (player, got)
    print(f't12: {player} notes in both implementations')
elif scen in ('t13', 't13third', 't14'):
    def go(picks, ties):
        soup = S.Soup()
        if scen.startswith('t13'):
            for P in (1, 2):
                for k in ('A2', 'E2', 'C3', 'E3', 'A3'):
                    S.install_key(soup, P, k, 'piano', ('const', 1.0))
            S.play(soup, 1, [[('A3', 'q')], [('C3', 'q')], [('E3', 'q')],
                             [('A3', 'h'), ('C3', 'h'), ('E3', 'h')]], None, Fraction(0))
            S.play(soup, 2, [[('A2', 'h')], [('E2', 'h')]], None, Fraction(0))
            if scen == 't13third':
                S.play(soup, 1, [[('A3', 'q')]], None, Fraction(0))
        else:
            w_v = {'e': 9.0, 'q': 1.0, 'h': 1.0}
            w_s = {'e': 1.0, 'q': 1.0, 'h': 9.0}
            psi = {'vibes': ('table', 'dur', {S.Dn(d): w for d, w in w_v.items()}),
                   'strings': ('table', 'dur', {S.Dn(d): w for d, w in w_s.items()})}
            for k in ('A3', 'C3', 'E3'):
                for tb in ('vibes', 'strings'):
                    S.install_key(soup, 1, k, tb, psi[tb])
            S.play(soup, 1, [[(k, 'e')] for _ in range(30) for k in ('A3', 'C3', 'E3')],
                   None, Fraction(0), tau=S.WILD)
        n = drive(soup, events, with_carry=False, picks=picks, ties=ties)
        left = [l for l in soup.live_locations() if soup.candidates(l)]
        assert not left, 'skeinsim has more to play than the player recorded'
        return n

    def search(picks):
        """Depth-first over the tie-breaks: some resolution of the ties must
        reproduce the whole trace."""
        ties = []
        try:
            return S._with_named_durations(lambda: go(picks, ties)), picks
        except AssertionError as err:
            last = err
        for step, k in ties:
            if step in picks: continue
            for i in range(1, k):
                r = search({**picks, step: i})
                if r: return r
            return None
        return None
    r = search({})
    assert r, 'no resolution of the tied alternatives reproduces the trace'
    n, picks = r
    print(f'{scen}: {n} resolutions agree (notes, timbres, onsets, alternative counts), including every null note'
          + (f'; {len(picks)} tie(s) between records differing only in payload process' if picks else ''))
else:
    sys.exit('unknown scenario')
