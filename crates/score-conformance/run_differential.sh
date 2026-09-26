#!/bin/sh
# The cross-implementation differential (spec, "Additional tests"): the Rust
# player and skeinsim.py, driven by the same recorded choices, emit the same
# records. Run from the workspace root:
#     crates/score-conformance/run_differential.sh PATH/TO/publications/f1r3score
set -e
PUB=${1:?usage: run_differential.sh PUBLICATIONS/f1r3score}
cargo build --release -q -p f1r3score
B=target/release/f1r3score
S=crates/score-conformance/scores
D=crates/score-conformance/differential.py
T=$(mktemp -d)
$B play $S/t1_law.score --stream A=prng:7 --gc freshness --notes 2000 --trace $T/t1.jsonl --quiet 2>/dev/null
$B play $S/t5_dual.score --stream A=prng:5 --gc freshness --notes 2000 --trace $T/t5.jsonl --quiet 2>/dev/null
$B play $S/t12_twinkle.score --trace $T/t12.jsonl --quiet 2>/dev/null
$B play $S/t13_fourhands.score --trace $T/t13.jsonl --quiet 2>/dev/null
$B play $S/t14_chimera_small.score --chance prng:9 --trace $T/t14.jsonl --quiet 2>/dev/null
for x in t1 t5 t12 t13 t14; do python3 $D "$PUB" $T/$x.jsonl $x; done
for seed in 1 2 3 4 5 6; do
  $B play $S/t13_third.score --scheduler random:$seed --trace $T/t13third.jsonl --quiet 2>/dev/null
  python3 $D "$PUB" $T/t13third.jsonl t13third
done
rm -rf $T
