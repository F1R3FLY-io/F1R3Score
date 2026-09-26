use score_syntax::*;

pub const TWINKLE: &str = r#"
score Twinkle
import std
pitches   { r, C3, F3, G3, C4, D4, E4, F4, G4, A4 }
durations { q = 1/4, h = 1/2 }
timbres   { piano = gm(0) on 1, bass = gm(32) on 2 }

play line(base "Lm", piano) [ C4 q, C4 q, G4 q, G4 q, A4 q, A4 q, G4 h,
                              F4 q, F4 q, E4 q, E4 q, D4 q, D4 q, C4 h ]
   | line(base "Lb", bass)  [ C3 h, C3 h, F3 h, C3 h, F3 h, C3 h, G3 h, C3 h ]
"#;

fn roundtrip(src: &str) {
    let s = load("t.score", src, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    let printed = print_score(&s);
    let f2 = parse("printed.score", &printed, &mut NoFiles).unwrap_or_else(|e| panic!("{e}\n{printed}"));
    let mut arena = s.arena;
    let mut clauses = s.clauses;
    let alph2 = score_syntax::elab::alphabets(&f2).unwrap();
    assert_eq!(alph2, s.alph);
    let (init2, _, _, _) = score_syntax::elab::elaborate_into(&f2, &alph2, &mut arena, &mut clauses)
        .unwrap_or_else(|e| panic!("{e}\n{printed}"));
    let mut a: Vec<_> = s.initial.clone();
    let mut b = init2;
    a.sort();
    b.sort();
    assert_eq!(a, b, "print/parse round trip changed the term:\n{printed}");
}

#[test]
fn twinkle_parses_and_roundtrips() {
    let s = load("twinkle.score", TWINKLE, &mut NoFiles).unwrap();
    assert_eq!(s.initial.len(), 1);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    roundtrip(TWINKLE);
}

const VOICE: &str = r#"
import std
pitches { r, C4, D4, E4, G4, A4 }
durations { q = 1/4, e = 1/8 }
timbres { piano = gm(0) on 1 }
factor stepwise = table step { -2..2: 3, _: 3/10 }
factor country = machine pitch { C4 -> D4 @ 2, C4 -> E4 @ 3, D4 -> E4 @ 1, E4 -> C4 @ 1, D4 -> C4 @ 1 }
factor rhythm = machine dur uniform
play Voice(piano, country * stepwise * rhythm, C4, q, "A") #A
"#;

#[test]
fn voice_elaborates_and_roundtrips() {
    let s = load("v.score", VOICE, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    assert!(s.arena.len() > 20);
    roundtrip(VOICE);
}

#[test]
fn recursion_is_rejected() {
    let src = r#"
pitches { r, C4 } durations { q = 1/4 } timbres { piano }
def Loop(L: proc) = for (_ <- <@L, piano, C4>) { Loop(L) }
play Loop(0)
"#;
    let e = load("r.score", src, &mut NoFiles).err().expect("must fail");
    assert!(e.msg.contains("reflection"), "{e}");
}

#[test]
fn overlapping_alphabets_rejected() {
    let src = "pitches { r, C4 } durations { C4 = 1/4 } timbres { piano } play 0";
    let e = load("o.score", src, &mut NoFiles).err().unwrap();
    assert!(e.msg.contains("both"), "{e}");
}

#[test]
fn ill_sorted_and_undeclared() {
    let src = "import std pitches { r, C4 } durations { q = 1/4 } timbres { piano } play N(0, piano, q, q; 0)";
    assert!(load("s.score", src, &mut NoFiles).is_err());
    let src = "import std pitches { r, C4 } durations { q = 1/4 } timbres { piano } play N(0, flute, C4, q; 0)";
    assert!(load("s.score", src, &mut NoFiles).is_err());
}

#[test]
fn unrolled_voice_is_linear() {
    // k written steps: every hand continues with the next step; with memoised
    // expansion the elaborated term grows linearly in k.
    let mut src = String::from(
        "import std\npitches { r, C4, D4, E4, F4, G4, A4, B4, C5 }\ndurations { q = 1/4, h = 1/2 }\ntimbres { piano }\n\
         factor psi = machine pitch uniform * machine dur uniform\ndef U0(w: name) = 0\n",
    );
    let k = 200;
    for i in 1..=k {
        src.push_str(&format!("def U{i}(w: name) = Step(w, piano, psi, U{}(_))\n", i - 1));
    }
    src.push_str(&format!("play U{k}(<@Seed(base \"A\", piano, C4, q), piano, C4>)\n"));
    let s = load("u.score", &src, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    assert!(s.arena.len() < 400 * k, "arena has {} nodes", s.arena.len());
}

#[test]
fn chord_and_behavioural_clause_parse() {
    let src = r#"
pitches { r, C4, E4, G4 } durations { q = 1/4 } timbres { piano }
play for (a <- <@base "K", piano, C4> & b <- <@base "K", piano, E4>
          where [carry#1(G4) && !carry#2(G4)] * [leads(nu X. <!last[C4, C4]> X)]) { 0 }
"#;
    let s = load("c.score", src, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    assert!(s.behavioural);
    roundtrip(src);
}

#[test]
fn graded_modalities_are_rejected_with_reason() {
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { piano } play for (_ <- <@0, piano, C4> where <> 1) { 0 }";
    let e = load("g.score", src, &mut NoFiles).err().unwrap();
    assert!(e.msg.contains("checkable fragment"), "{e}");
}

fn cmp(a: &score_core::Arena, x: score_core::ProcId, y: score_core::ProcId, depth: usize) {
    use score_core::Proc;
    if x == y { return; }
    match (a.get(x), a.get(y)) {
        (Proc::Par(p), Proc::Par(q)) if p.len() == q.len() => {
            let only_p: Vec<_> = p.iter().filter(|c| !q.contains(c)).collect();
            let only_q: Vec<_> = q.iter().filter(|c| !p.contains(c)).collect();
            if only_p.len() == 1 && only_q.len() == 1 { return cmp(a, *only_p[0], *only_q[0], depth + 1); }
            panic!("par differs at depth {depth}: {} vs {}", only_p.len(), only_q.len());
        }
        (Proc::Recv { subjects: s1, clause: c1, body: b1, label: l1 }, Proc::Recv { subjects: s2, clause: c2, body: b2, label: l2 }) => {
            if s1 != s2 { panic!("subjects differ at depth {depth}: {s1:?} {s2:?}"); }
            if c1 != c2 { panic!("clauses differ at depth {depth}: {c1:?} {c2:?}"); }
            if l1 != l2 { panic!("labels differ at depth {depth}: {l1:?} {l2:?}"); }
            cmp(a, *b1, *b2, depth + 1)
        }
        (Proc::Send { subj: s1, payload: p1, .. }, Proc::Send { subj: s2, payload: p2, .. }) => {
            if p1 != p2 { return cmp(a, *p1, *p2, depth + 1); }
            panic!("send subjects differ at depth {depth}: {s1:?} {s2:?}");
        }
        (p, q) => panic!("differ at depth {depth}: {p:?} vs {q:?}"),
    }
}

#[test]
fn voice_same_arena_structural() {
    let s = load("v.score", VOICE, &mut NoFiles).unwrap();
    let printed = print_score(&s);
    let f2 = parse("printed.score", &printed, &mut NoFiles).unwrap();
    let mut arena = s.arena;
    let mut clauses = s.clauses;
    let (init2, _, _, _) = score_syntax::elab::elaborate_into(&f2, &s.alph, &mut arena, &mut clauses).unwrap();
    cmp(&arena, s.initial[0].1, init2[0].1, 0);
}

// ------------------------------------------------------------- revision 2

const FOUR_HANDS: &str = r#"
score FourHands
import std
pitches   { r, A2, E2, C3, E3, A3 }
durations { q = 1/4, h = 1/2 }
timbres   { piano = gm(0) on 1 }
def P1 = base "P1"   def P2 = base "P2"
play Keyboard(P1, piano, 1, {A2, E2, C3, E3, A3})
   | Keyboard(P2, piano, 1, {A2, E2, C3, E3, A3})
   | touch(P1, A3, q)!(0) ; touch(P1, C3, q)!(0) ; touch(P1, E3, q)!(0) ;
       ( touch(P1, A3, h)!(0) | touch(P1, C3, h)!(0) | touch(P1, E3, h)!(0) ) ; 0
   | touch(P2, A2, h)!(0) ; touch(P2, E2, h)!(0) ; 0
"#;

#[test]
fn four_hands_parses_without_warnings_and_roundtrips() {
    let s = load("fh.score", FOUR_HANDS, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    // keyloc, keycode and the acknowledgement locations print as builtins
    let printed = print_score(&s);
    assert!(printed.contains("keyloc(") && printed.contains("keycode(") && printed.contains("ackloc("));
    assert!(!printed.contains("dead"), "the dead timbre never appears in printed user syntax");
    roundtrip(FOUR_HANDS);
}

#[test]
fn wildcards_general_names_and_sequencing_parse() {
    let src = r#"
pitches { r, C4 } durations { q = 1/4 } timbres { piano, vibes }
play for (y <- <@base "K", _, C4>) { *y }
   | <@base "K", piano, q>!(0)
   | <@base "K", vibes, q>!(0, piano, _)
   | @(base "J")!(0)
"#;
    let s = load("w.score", src, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    roundtrip(src);
    // `x!(Q)` passes the general name @Q
    let printed = print_score(&s);
    assert!(printed.contains("!(0)"), "{printed}");
    // `;` binds more tightly than `|`: two players in parallel
    let src2 = r#"
import std
pitches { r, C4, E4 } durations { q = 1/4 } timbres { piano }
def P = base "P"
play Keyboard(P, piano, 1, pitches-r) | touch(P, C4, q)!(0) ; touch(P, E4, q)!(0) ; 0 | touch(P, E4, q)!(0) ; 0
"#;
    load("s.score", src2, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn the_dead_timbre_cannot_be_written_and_reserved_names_cannot_be_declared() {
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { piano } play <@0, dead, q>!(0)";
    let e = load("d.score", src, &mut NoFiles).err().unwrap();
    assert!(e.msg.contains("dead"), "{e}");
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { ctl } play 0";
    let e = load("c.score", src, &mut NoFiles).err().unwrap();
    assert!(e.msg.contains("reserved"), "{e}");
}

#[test]
fn a_sequence_under_a_payload_is_rejected() {
    let src = r#"
import std
pitches { r, C4 } durations { q = 1/4 } timbres { piano }
def P = base "P"
play <@base "S", piano, q>!(touch(P, C4, q)!(0) ; 0)
"#;
    let e = load("p.score", src, &mut NoFiles).err().unwrap();
    assert!(e.msg.contains("payload"), "{e}");
}

#[test]
fn open_subjects_are_linted() {
    // a subject with datum `_` has no polarity
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { piano } play for (_ <- <@0, piano, _>) { 0 } | <@0, piano, q>!(0)";
    let s = load("l.score", src, &mut NoFiles).unwrap();
    assert!(s.warnings.iter().any(|w| w.msg.contains("no polarity")), "{:?}", s.warnings);
    // both timbres open with nothing else there
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { piano } play for (_ <- <@0, _, C4>) { 0 } | <@0, _, q>!(0)";
    let s = load("l2.score", src, &mut NoFiles).unwrap();
    assert!(s.warnings.iter().any(|w| w.msg.contains("open")), "{:?}", s.warnings);
    // revision 1's payload-timbre warning is withdrawn
    let src = "pitches { r, C4 } durations { q = 1/4 } timbres { piano, vibes } play for (_ <- <@0, piano, C4>) { 0 } | <@0, piano, q>!(0, vibes, C4)";
    let s = load("l3.score", src, &mut NoFiles).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

#[test]
fn patterns_distinguish_any_from_the_wildcard() {
    use score_logic::*;
    let src = r#"
pitches { r, C4 } durations { q = 1/4 } timbres { piano }
play for (_ <- <@0, piano, C4> where [passes(<?, ?, ?>!(true, _, _))] * [!carry(_)]) { 0 } | <@0, piano, q>!(0)
"#;
    let s = load("p.score", src, &mut NoFiles).unwrap_or_else(|e| panic!("{e}"));
    roundtrip(src);
    let mut a = s.arena;
    let nil = a.nil();
    let t = score_core::Hat::Is(score_core::Timbre(0));
    let c4 = score_core::Hat::Is(s.alph.lookup_datum("C4").unwrap());
    let general = a.send_general(a.quote(nil, t, c4), nil);
    let decorated = a.send(a.quote(nil, t, c4), nil, t, c4);
    let pat = Sp::Out {
        subj: NamePat::any(),
        payload: Box::new(Sp::True),
        ptimbre: PatC::Wild,
        carry: PatC::Wild,
    };
    let mut ev = Evaluator::new(&a, &s.alph);
    assert!(ev.sp(&pat, general), "`_` matches a wildcard");
    assert!(!ev.sp(&pat, decorated), "`_` matches only a wildcard");
    let any = Sp::Out { subj: NamePat::any(), payload: Box::new(Sp::True), ptimbre: PatC::Any, carry: PatC::Any };
    assert!(ev.sp(&any, general) && ev.sp(&any, decorated), "`?` matches both");
}
