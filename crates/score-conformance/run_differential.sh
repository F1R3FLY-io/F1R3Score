#!/bin/sh
# The cross-implementation differential (spec, "Additional tests"): the Rust
# player and skeinsim.py, driven by the same recorded choices, emit the same
# records -- notes, carried data, timbres, onsets and numbers of alternatives,
# null notes included -- for T0-T8 and T10-T14, and for random small scores.
# Run from the workspace root:
#     crates/score-conformance/run_differential.sh PATH/TO/publications/f1r3score [N_RANDOM]
set -e
PUB=${1:?usage: run_differential.sh PUBLICATIONS/f1r3score [N_RANDOM]}
NR=${2:-40}
cargo build --release -q -p f1r3score
B=target/release/f1r3score
S=crates/score-conformance/scores
D=crates/score-conformance/differential.py
T=$(mktemp -d)
trap 'rm -rf $T' EXIT
fail=0
check() {  # SCENARIO SCORE PLAYER-OPTIONS...
  scen=$1; score=$2; shift 2
  $B play "$score" "$@" --trace $T/run.jsonl --quiet 2>/dev/null
  if ! python3 $D "$PUB" $T/run.jsonl "$scen"; then echo "DIFFERS: $scen ($score)"; fail=1; fi
}
V="--stream A=prng:7 --gc freshness --notes 300"
check t0 $S/t0_inertness.score $V
check t1 $S/t1_law.score --stream A=prng:7 --gc freshness --notes 2000
check t2 $S/t2_harmony.score --stream A=prng:11 --stream B=prng:12 --gc freshness --notes 300 --scheduler random:5
check t3 $S/t3_two_sided.score --chance prng:3
check t3 $S/t3_two_sided.score --chance prng:4 --mode one
check t4 $S/t4_embedding.score --stream A.pitch=spigot:pi/16@1 --stream A.dur=spigot:e/5 --gc freshness --notes 200
check t5 $S/t5_dual.score --stream A=prng:5 --gc freshness --notes 2000
check t6 $S/t6_coupled.score --chance prng:6
check t7stepwise $S/t7_stepwise.score $V
check t7leaping $S/t7_leaping.score $V
check t8unconstrained $S/t8_unconstrained.score $V
check t8forbid $S/t8_forbid.score $V
check t8motif $S/t8_motif.score $V
check t10 $S/t10_reflective.score --stream A=prng:99 --gc freshness --notes 300
check t11 $S/t11_reflective_inert.score --stream A=prng:1 --notes 200
check t12 $S/t12_twinkle.score
check t13 $S/t13_fourhands.score
for seed in 1 2 3 4 5 6; do check t13third $S/t13_third.score --scheduler random:$seed; done
check t14 $S/t14_chimera_small.score --chance prng:9
i=0
while [ $i -lt $NR ]; do
  python3 $D "$PUB" --gen random:$i $T/random.score
  check random:$i $T/random.score --chance prng:$i --stream A=prng:$((i+100)) --stream B=prng:$((i+200)) \
        --scheduler random:$i --notes 120
  i=$((i+1))
done
[ $fail -eq 0 ] && echo "differential: every scenario agrees" || { echo "differential: DISAGREEMENTS"; exit 1; }
