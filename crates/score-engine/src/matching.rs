//! Maximal matchings of a contention set, in canonical (lexicographic) order.

use crate::soup::Cand;
use crate::EngineError;

fn conflict(a: &Cand, b: &Cand) -> bool {
    a.recv == b.recv || a.sends.iter().any(|s| b.sends.contains(s))
}

/// All maximal sets of pairwise non-contending candidates, as sorted index
/// lists in lexicographic order. A star (all candidates share one receipt)
/// takes the fast path: its maximal matchings are the single candidates.
pub fn maximal_matchings(cands: &[Cand], bound: usize) -> Result<Vec<Vec<usize>>, EngineError> {
    let n = cands.len();
    if n == 0 {
        return Ok(vec![]);
    }
    if cands.iter().all(|c| c.recv == cands[0].recv) {
        if n > bound {
            return Err(EngineError::alternatives(bound));
        }
        return Ok((0..n).map(|i| vec![i]).collect());
    }
    let adj: Vec<Vec<bool>> = (0..n).map(|i| (0..n).map(|j| i != j && conflict(&cands[i], &cands[j])).collect()).collect();
    let mut out = vec![];
    let mut chosen: Vec<usize> = vec![];
    // include-first DFS enumerates in lexicographic order of sorted index lists
    fn dfs(
        i: usize,
        n: usize,
        adj: &[Vec<bool>],
        chosen: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
        bound: usize,
    ) -> Result<(), EngineError> {
        if i == n {
            // maximal: every unchosen candidate conflicts with a chosen one
            let maximal = (0..n).all(|j| chosen.contains(&j) || chosen.iter().any(|&c| adj[c][j]));
            if maximal {
                out.push(chosen.clone());
                if out.len() > bound {
                    return Err(EngineError::alternatives(bound));
                }
            }
            return Ok(());
        }
        if chosen.iter().all(|&c| !adj[c][i]) {
            chosen.push(i);
            dfs(i + 1, n, adj, chosen, out, bound)?;
            chosen.pop();
        }
        // excluding i is only useful if something later (or earlier) can block it
        let blocked_now = chosen.iter().any(|&c| adj[c][i]);
        let blockable_later = (i + 1..n).any(|j| adj[i][j]);
        if blocked_now || blockable_later {
            dfs(i + 1, n, adj, chosen, out, bound)?;
        }
        Ok(())
    }
    dfs(0, n, &adj, &mut chosen, &mut out, bound)?;
    Ok(out)
}
