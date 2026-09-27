#!/usr/bin/env python3
"""Cross-implementation differential (spec, "Additional tests"): drive
skeinsim.py with the choices the Rust player recorded in a trace, and check
that both implementations emit the same records -- the same notes, carried
data and payload decorations, with the same number of alternatives -- at every
resolution.

    python3 differential.py PUBLICATIONS/f1r3score TRACE.jsonl SCENARIO
    python3 differential.py PUBLICATIONS/f1r3score --gen random:SEED OUT.score

SCENARIO is one of t0 t1 t2 t3 t4 t5 t6 t7stepwise t7leaping t8unconstrained
t8forbid t8motif t10 t11 t12 t13 t13third t14, or random:SEED.  The trace must
come from the matching score in scores/ (run_differential.sh pairs them).  T9
compares a table with its formula on views, not a run, and has no trace.
`--gen random:SEED` writes a random small score -- a voice or two, or a
keyboard or chimera with random touches -- and random:SEED rebuilds the same
configuration in skeinsim.

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

GEN = sys.argv[2] == '--gen'
if not GEN:
    trace = [json.loads(l) for l in open(sys.argv[2]) if l.strip()]
    scen = sys.argv[3]
    head, events = trace[0], trace[1:]
    P_NAMES = [p[0] for p in head['pitches']]
    D_NAMES = [d[0] for d in head['durations']]
    POS_D = [d[0] for d in head['durations'] if Fraction(d[1]) > 0]
    MODE = head.get('mode', 'max')


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


def drive(soup, evs, with_carry=True, picks=None, ties=None, ticks=1, gc=False):
    """Force skeinsim through the recorded events. Where several of its
    alternatives emit exactly the recorded records (they differ only in the
    payload process, which the two implementations represent incomparably),
    `picks[step]` chooses among them; the tied steps are appended to `ties`."""
    picks = picks or {}
    # a one-to-one correspondence between the player's locations (by digest)
    # and skeinsim's, built as locations are first used and respected after
    lmap, rmap = {}, {}
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
        # the stream that paid for a musical choice names the voice that made
        # it; skeinsim tags each voice's key receipts with the same label
        streams = set(e.get('digits', {})) - {'*'}
        streams = {x.split('.')[0] for x in streams}
        cands = []
        for t, loc, comp in comps:
            if t != tmin: continue
            if streams and len(streams) == 1:
                tag = comp[0][0].get('tag')
                if isinstance(tag, str) and tag in ('A', 'B') and tag not in streams:
                    continue
            # the trace's matching mode: a maximal matching, or one candidate
            opts = S.maximal_matchings(comp) if MODE == 'max' else [[c] for c in comp]
            for S_ in opts:
                got = sorted(key_of(rec_skein(loc, c[0], c[1]), with_carry) for c in S_)
                if got == want:
                    ploc = e['records'][0]['loc']
                    if lmap.get(ploc, loc) != loc or rmap.get(loc, ploc) != ploc:
                        continue      # inconsistent with the correspondence
                    cands.append((loc, comp, S_, len(opts)))
        assert cands, f'step {e["step"]}: no due set in skeinsim emits {want}'
        if len(cands) > 1 and ties is not None:
            ties.append((e['step'], len(cands)))
        loc, comp, S_, nalts = cands[picks.get(e['step'], 0) % len(cands)]
        ploc = e['records'][0]['loc']
        lmap[ploc] = loc; rmap[loc] = ploc
        assert nalts == e['alts'], f'step {e["step"]}: alternatives: skeinsim {nalts}, player {e["alts"]}'
        for (r, m, w, note) in S_:
            soup.remove(loc, r, m)
            onset = max(r['stamp'], m['stamp'])
            z = (m['payload'], m['ptau'], m['carry'])
            r['cont'](soup, z, onset + S.dur_value(note[1]))
        # collect record locations once nothing there has a candidate
        # (voices only: skeinsim's key locations are opaque nodes)
        if gc and S.node(loc[0])[0] == 'send' and not soup.candidates(loc):
            soup.M.pop(loc, None); soup.R.pop(loc, None)
        n += 1
    return n



# ----------------------------------------------------------- configurations
# Each builder returns (soup, options) for skeinsim, constructed exactly as
# gen_scores.py wrote the player's score: same machines, same seeds.

def refl(soup, tag, psi, NP, ND, p0, u0):
    S.reflective_voice(soup, tag, 'piano', psi, NP, ND, p0, u0)

def voice_from(seed_, deg_d, ep_deg=(2, 4), p0=0, u0=2):
    rng = random.Random(seed_)
    EP, ED = S.random_machine(16, rng, ep_deg), S.random_machine(5, rng, deg_d)
    soup = S.Soup(); refl(soup, 'A', S.machine_clause(EP, ED), 16, 5, p0, u0)
    return soup

def t2():
    r0 = random.Random(3)
    EA, EDA = S.random_machine(16, r0), S.random_machine(5, r0, (1, 3))
    EB, EDB = S.random_machine(16, r0), S.random_machine(5, r0, (1, 3))
    soup = S.Soup()
    refl(soup, 'A', S.machine_clause(EA, EDA), 16, 5, 0, 2)
    refl(soup, 'B', S.machine_clause(EB, EDB), 16, 5, 7, 1)
    return soup

def fitting(tag, keys, n):
    soup = S.Soup(); H = S.base(tag)
    for i in keys:
        soup.receipt(H, 'piano', S.P(i), ('const', 1.0), lambda s, z, t: None, 0, tag)
    for j in range(n):
        soup.send(H, 'piano', S.D(j), S.base(('q', j)), 'piano', S.P(0))
    return soup

def t4():
    EP = {(s, t): 1.0 for s in range(16) for t in range(16)}
    ED = {(u, v): 1.0 for u in range(5) for v in range(5)}
    soup = S.Soup(); refl(soup, 'A', S.machine_clause(EP, ED), 16, 5, 2, 0)
    return soup

def t5():
    rng = random.Random(7)
    EP, ED = S.random_machine(16, rng), S.random_machine(5, rng, (2, 3))
    soup = S.Soup()
    S.voice(soup, S.seed('A', 'piano', 0, 2, dual=True), 0, S.dual_clause(EP, ED), 16, 5, 'A', dual=True)
    return soup

def t7(kind):
    rng = random.Random(21); NP, ND = 21, 5
    style = S.random_machine(NP, rng, (4, 7))
    ED = {(u, v): 1.0 for u in range(ND) for v in (2, 3)}
    fdur = ('table', 'prev,dur', {(S.D(u), S.D(v)): w for (u, v), w in ED.items()})
    fsty = ('table', 'pitch,carry', {(S.P(s), S.P(t)): w for (s, t), w in style.items()})
    f = {k: (3.0 if (abs(k) <= 2 if kind == 'stepwise' else abs(k) in (4, 7)) else 0.3) for k in range(-NP, NP)}
    soup = S.Soup(); refl(soup, 'A', ('tensor', fsty, ('table', 'step', f), fdur), NP, ND, 0, 2)
    return soup

def t8(kind):
    rng = random.Random(33)
    EP, ED = S.random_machine(16, rng, (3, 5)), S.random_machine(5, rng, (2, 3))
    EP2 = dict(EP); EP2[(0, 2)] = EP2.get((0, 2), 1); EP2[(2, 4)] = EP2.get((2, 4), 1)
    EP2[(2, 2)] = EP2.get((2, 2), 1); EP2[(5, 5)] = 3
    base2 = S.machine_clause(EP2, ED)
    forbid = ('crisp', ('not', ('repeat',)))
    motif = ('sum', [('crisp', ('and', ('back', 1, S.P(0)), ('back', 0, S.P(2)), ('carry', S.P(4)))), ('const', 0.0)])
    boost = ('sum', [('tensor', motif, ('const', 20.0)), ('const', 1.0)])
    psi = {'unconstrained': base2, 'forbid': ('tensor', base2, forbid), 'motif': ('tensor', base2, boost)}[kind]
    soup = S.Soup(); refl(soup, 'A', psi, 16, 5, 0, 2)
    return soup

def t10():
    rng0 = random.Random(7)
    EP, ED = S.random_machine(16, rng0), S.random_machine(5, rng0, (2, 3))
    soup = S.Soup(); refl(soup, 'A', S.machine_clause(EP, ED), 16, 5, 0, 2)
    return soup

def t13(third):
    soup = S.Soup()
    for P in (1, 2):
        for k in ('A2', 'E2', 'C3', 'E3', 'A3'):
            S.install_key(soup, P, k, 'piano', ('const', 1.0))
    S.play(soup, 1, [[('A3', 'q')], [('C3', 'q')], [('E3', 'q')], [('A3', 'h'), ('C3', 'h'), ('E3', 'h')]], None, Fraction(0))
    S.play(soup, 2, [[('A2', 'h')], [('E2', 'h')]], None, Fraction(0))
    if third:
        S.play(soup, 1, [[('A3', 'q')]], None, Fraction(0))
    return soup

def chimera_soup(keys, tables, phrase, tau):
    soup = S.Soup()
    for k in keys:
        for tb, table in tables.items():
            S.install_key(soup, 1, k, tb, ('table', 'dur', {S.Dn(d): w for d, w in table.items()}))
    S.play(soup, 1, phrase, None, Fraction(0), tau=tau)
    return soup

def t14():
    return chimera_soup(('A3', 'C3', 'E3'), {'vibes': {'e': 9.0, 'q': 1.0, 'h': 1.0}, 'strings': {'e': 1.0, 'q': 1.0, 'h': 9.0}},
                        [[(k, 'e')] for _ in range(30) for k in ('A3', 'C3', 'E3')], S.WILD)

# --------------------------------------------------------- random small scores
CH = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B']
DUR = [('w', '1'), ('h', '1/2'), ('q', '1/4'), ('e', '1/8'), ('s', '1/16')]

def random_case(seed_):
    """A random small configuration, as a description both sides build from."""
    rng = random.Random(1000 + seed_)
    if rng.random() < 0.5:
        NP, ND = rng.randint(3, 7), rng.randint(2, 4)
        voices = []
        for tag in ('A', 'B')[:rng.randint(1, 2)]:
            EP = S.random_machine(NP, rng, (1, min(3, NP)))
            ED = S.random_machine(ND, rng, (1, min(2, ND)))
            voices.append((tag, EP, ED, rng.randrange(NP), rng.randrange(ND)))
        return ('voices', NP, ND, voices)
    keys = ['C4', 'E4', 'G4', 'A4'][:rng.randint(2, 4)]
    timbres = ['vibes', 'strings'][:rng.randint(1, 2)]
    tables = {tb: {d: float(rng.choice([1, 2, 3, 9])) for d in ('e', 'q', 'h')} for tb in timbres}
    phrase = []
    for _ in range(rng.randint(4, 12)):
        ks = rng.sample(keys, rng.randint(1, min(2, len(keys))))
        phrase.append([(k, rng.choice(['e', 'q', 'h'])) for k in ks])
    named = rng.random() < 0.3
    return ('keys', keys, timbres, tables, phrase, timbres[0] if named else S.WILD)

def random_score(seed_):
    c = random_case(seed_)
    head = f"// random small score {seed_} for the differential (differential.py --gen)\nimport std\n"
    if c[0] == 'voices':
        _, NP, ND, voices = c
        P = [CH[i] + '4' for i in range(NP)]
        out = head + f"pitches {{ {', '.join(P)}, r }}\ndurations {{ {', '.join(f'{a} = {b}' for a, b in DUR[:ND])} }}\ntimbres {{ piano }}\n"
        items = []
        for tag, EP, ED, p0, u0 in voices:
            ep = ', '.join(f"{P[s]} -> {P[t]} @ {w}" for (s, t), w in sorted(EP.items()))
            ed = ', '.join(f"{DUR[s][0]} -> {DUR[t][0]} @ {w}" for (s, t), w in sorted(ED.items()))
            out += f"factor style{tag} = machine pitch {{ {ep} }}\nfactor rhythm{tag} = machine dur {{ {ed} }}\n"
            items.append(f'Voice(piano, style{tag} * rhythm{tag}, {P[p0]}, {DUR[u0][0]}, "{tag}") #{tag}')
        return out + "play " + "\n   | ".join(items) + "\n"
    _, keys, timbres, tables, phrase, tau = c
    out = head + f"pitches {{ r, {', '.join(keys)} }}\ndurations {{ e = 1/8, q = 1/4, h = 1/2 }}\n"
    out += f"timbres {{ {', '.join(timbres)} }}\ndef P = base \"P1\"\n"
    for tb in timbres:
        out += f"factor t_{tb} = table dur {{ {', '.join(f'{d}: {int(w)}' for d, w in tables[tb].items())} }}\n"
    voices = ', '.join(f'({tb}, t_{tb})' for tb in timbres)
    touch = (lambda k, d: f"touch(P, {k}, {d})!(0)") if tau == S.WILD else (lambda k, d: f"<@keyloc(P, {k}), {tau}, {d}>!(0)")
    steps = [touch(*st[0]) if len(st) == 1 else '(' + ' | '.join(touch(k, d) for k, d in st) + ')' for st in phrase]
    return out + f"play ChimeraKeyboard(P, {{{', '.join(keys)}}}, [{voices}])\n   | " + ' ; '.join(steps) + ' ; 0\n'

def random_soup(seed_):
    c = random_case(seed_)
    if c[0] == 'voices':
        _, NP, ND, voices = c
        soup = S.Soup()
        for tag, EP, ED, p0, u0 in voices:
            refl(soup, tag, S.machine_clause(EP, ED), NP, ND, p0, u0)
        return soup, False
    _, keys, timbres, tables, phrase, tau = c
    return chimera_soup(keys, tables, phrase, tau), True

if GEN:
    seed_ = int(sys.argv[3].split(':')[1])
    open(sys.argv[4], 'w').write(random_score(seed_))
    sys.exit(0)

# -------------------------------------------------------------------- run
VOICES = {
    't0': lambda: voice_from(1, (1, 3)),
    't1': lambda: voice_from(7, (2, 3)),
    't2': t2, 't4': t4, 't10': t10,
    't7stepwise': lambda: t7('stepwise'), 't7leaping': lambda: t7('leaping'),
    't8unconstrained': lambda: t8('unconstrained'), 't8forbid': lambda: t8('forbid'), 't8motif': lambda: t8('motif'),
    't11': lambda: voice_from(1, (2, 3)),
}
FITTINGS = {'t3': lambda: fitting('K', range(3), 4), 't6': lambda: fitting('shared', (0, 4), 5)}
INSTRUMENTS = {'t13': lambda: t13(False), 't13third': lambda: t13(True), 't14': t14}

def compare(build, instrument, ticks, gc, with_carry, evs, label):
    def once(picks, ties):
        soup = build()
        n = drive(soup, evs, with_carry=with_carry, picks=picks, ties=ties, ticks=ticks, gc=gc)
        if instrument or scen in FITTINGS:
            left = [l for l in soup.live_locations() if soup.candidates(l)]
            assert not left, 'skeinsim has more to play than the player recorded'
        return n

    def search(picks, budget=[400]):
        """Depth-first over the tie-breaks, each tie assigned in turn (index 0
        included): some assignment must reproduce the whole trace."""
        ties = []
        budget[0] -= 1
        if budget[0] < 0:
            raise AssertionError('tie search exhausted: treat as a disagreement')
        try:
            return once(picks, ties), picks
        except AssertionError:
            pass
        free = [(st, k) for st, k in ties if st not in picks]
        if not free:
            return None
        step, k = free[0]
        for i in range(k):
            r = search({**picks, step: i})
            if r:
                return r
        return None

    run = (lambda: search({})) if not instrument else (lambda: S._with_named_durations(lambda: search({})))
    r = run()
    assert r, 'no resolution of the tied alternatives reproduces the trace'
    n, picks = r
    print(f'{label}: {n} resolutions agree ({"notes, timbres" if instrument else "notes, carried data"}, onsets, '
          f'alternative counts), including every null note'
          + (f'; {len(picks)} tie(s) between records differing only in payload process' if picks else ''))

if scen in VOICES:
    compare(VOICES[scen], False, 16, True, True, events, scen)
elif scen in FITTINGS:
    compare(FITTINGS[scen], False, 16, False, True, events, scen)
elif scen == 't5':
    # skeinsim has no reflective dual voice: compare the musical fittings
    evs = [e for e in events if any(rec_player(r)[2] in POS_D for r in e['records'])]
    n = drive(t5(), evs, ticks=16, gc=True)
    print(f't5: {n} fittings agree (notes, carried data, onsets, alternative counts)')
elif scen == 't12':
    assert all(e['alts'] == 1 for e in events)
    got = S.T12_twinkle()
    player = sum(len(e['records']) for e in events)
    assert player == got['notes'], (player, got)
    print(f't12: {player} notes in both implementations')
elif scen in INSTRUMENTS:
    compare(INSTRUMENTS[scen], True, 1, False, False, events, scen)
elif scen.startswith('random:'):
    seed_ = int(scen.split(':')[1])
    soup0, instrument = random_soup(seed_)
    kind = random_case(seed_)[0]
    compare(lambda: random_soup(seed_)[0], instrument, 1 if instrument else 16, not instrument, not instrument,
            events, f'{scen} ({kind})')
else:
    sys.exit('unknown scenario')
