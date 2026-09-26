//! `score-chance`: chance sources for the f1r3score player.
//!
//! A source is a stream of digits in a base; a choice among weighted
//! alternatives is drawn by exact lazy interval decoding (Algorithm 1 of the
//! drawn-distributions design): no floating point anywhere, and the number of
//! digits consumed is recorded. No I/O.

use num_bigint::BigUint;
use num_traits::{One, Zero};
use score_core::Q;

/// A stream of digits `0..base`.
pub trait DigitStream {
    fn base(&self) -> u64;
    fn next_digit(&mut self) -> u64;
    fn describe(&self) -> String;
}

// ------------------------------------------------------------------ PRNG

/// xoshiro256** seeded by SplitMix64 from a 64-bit seed. Digits are the high
/// 32 bits of each output, in base 2^32. A run is bit-for-bit reproducible
/// from its seed on every platform.
#[derive(Clone, Debug)]
pub struct Prng {
    s: [u64; 4],
    seed: u64,
}

impl Prng {
    pub fn new(seed: u64) -> Prng {
        let mut x = seed;
        let mut sm = || {
            x = x.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        };
        Prng { s: [sm(), sm(), sm(), sm()], seed }
    }
    pub fn next_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }
    /// A uniform integer in `0..n` by rejection (for the random scheduler).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0);
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let x = self.next_u64();
            if x < zone {
                return x % n;
            }
        }
    }
}

impl DigitStream for Prng {
    fn base(&self) -> u64 {
        1u64 << 32
    }
    fn next_digit(&mut self) -> u64 {
        self.next_u64() >> 32
    }
    fn describe(&self) -> String {
        format!("prng:{}", self.seed)
    }
}

// ---------------------------------------------------------------- spigots

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constant {
    Pi,
    E,
    Ln2,
    Liouville,
    Champernowne,
    ThueMorse,
}

impl Constant {
    pub fn parse(s: &str) -> Option<Constant> {
        Some(match s {
            "pi" => Constant::Pi,
            "e" => Constant::E,
            "ln2" => Constant::Ln2,
            "liouville" => Constant::Liouville,
            "champernowne" => Constant::Champernowne,
            "thue-morse" | "thuemorse" => Constant::ThueMorse,
            _ => return None,
        })
    }
    pub fn name(&self) -> &'static str {
        match self {
            Constant::Pi => "pi",
            Constant::E => "e",
            Constant::Ln2 => "ln2",
            Constant::Liouville => "liouville",
            Constant::Champernowne => "champernowne",
            Constant::ThueMorse => "thue-morse",
        }
    }
    /// How many leading digits of the stream are the integer part.
    pub fn integer_digits(&self, base: u64) -> usize {
        let int_part: u64 = match self {
            Constant::Pi => 3,
            Constant::E => 2,
            Constant::Ln2 | Constant::Liouville | Constant::Champernowne => 0,
            Constant::ThueMorse => return 0,
        };
        let mut n = 1;
        let mut x = int_part / base;
        while x > 0 {
            n += 1;
            x /= base;
        }
        n
    }
}

/// The fractional digits of a constant in a base, computed exactly.
///
/// pi, e and ln 2 are approximated by integer series with rigorous error
/// bounds at doubling precision, and a batch of digits is emitted only when
/// the lower and upper bounds agree on it, so every digit is exact. Liouville,
/// Champernowne and Thue-Morse digits are positional rules and exact by
/// construction. (The `spigot_stream` crate of F1R3Games was the intended
/// source; at the time of writing its non-decimal pi and decimal ln 2 streams
/// were defective, see DESIGN.md.)
#[cfg(feature = "spigot")]
pub struct Spigot {
    constant: Constant,
    base: u64,
    buf: Vec<u64>,
    pos: usize,
    /// for positional constants: the index of the next digit
    n: u64,
    champ: (u64, Vec<u64>),
    /// the driver position the stream was started at (for its description)
    pub start: u64,
}

#[cfg(feature = "spigot")]
mod series {
    use num_bigint::BigUint;
    use num_traits::{One, Zero};

    /// floor(2^p / x^k) etc. are computed in fixed point with `p` bits.
    fn atan_inv(x: u64, p: usize) -> (BigUint, BigUint, u64) {
        // sum_{k>=0} (-1)^k / ((2k+1) x^(2k+1)), returns (pos, neg, nterms)
        let one = BigUint::one() << p;
        let x2 = BigUint::from(x * x);
        let mut pow = &one / BigUint::from(x); // 2^p / x^(2k+1)
        let (mut pos, mut neg) = (BigUint::zero(), BigUint::zero());
        let mut k = 0u64;
        while !pow.is_zero() {
            let t = &pow / BigUint::from(2 * k + 1);
            if k % 2 == 0 {
                pos += t;
            } else {
                neg += t;
            }
            pow /= &x2;
            k += 1;
        }
        (pos, neg, k + 1)
    }

    /// (value * 2^p, error bound in ulps) for pi, e, ln2
    pub fn fixed(c: super::Constant, p: usize) -> (BigUint, BigUint) {
        use super::Constant::*;
        match c {
            Pi => {
                let (a, b, n1) = atan_inv(5, p);
                let (c2, d, n2) = atan_inv(239, p);
                let v = (a * 16u32 + d * 4u32) - (b * 16u32 + c2 * 4u32);
                (v, BigUint::from(20 * (n1 + n2) + 20))
            }
            E => {
                let mut term = BigUint::one() << p;
                let mut sum = BigUint::zero();
                let mut k = 1u64;
                while !term.is_zero() {
                    sum += &term;
                    term /= BigUint::from(k);
                    k += 1;
                }
                (sum, BigUint::from(k + 2))
            }
            Ln2 => {
                let mut sum = BigUint::zero();
                let mut k = 1u64;
                loop {
                    if k as usize > p {
                        break;
                    }
                    let t = (BigUint::one() << (p - k as usize)) / BigUint::from(k);
                    if t.is_zero() {
                        break;
                    }
                    sum += t;
                    k += 1;
                }
                (sum, BigUint::from(k + 2))
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(feature = "spigot")]
impl Spigot {
    pub fn new(constant: Constant, base: u64) -> Result<Spigot, String> {
        if !(2..=36).contains(&base) {
            return Err(format!("spigot bases are 2..36, not {base}"));
        }
        if constant == Constant::ThueMorse && base != 2 {
            return Err("the Thue-Morse constant is a binary expansion: use base 2".into());
        }
        Ok(Spigot { constant, base, buf: vec![], pos: 0, n: 0, champ: (1, vec![]), start: 0 })
    }

    /// Recompute the first `n` fractional digits exactly.
    fn refill(&mut self, n: usize) {
        let b = BigUint::from(self.base);
        let bn = num_traits::pow(b.clone(), n);
        let mut p = (n as f64 * (self.base as f64).log2()) as usize + 96;
        loop {
            let (v, err) = series::fixed(self.constant, p);
            let int = &v >> p;
            let frac_lo = (&v - &err) - (&int << p);
            let frac_hi = (&v + &err) - (&int << p);
            let lo = (&frac_lo * &bn) >> p;
            let hi = (&frac_hi * &bn) >> p;
            if lo == hi {
                let mut digits = vec![0u64; n];
                let mut x = lo;
                for i in (0..n).rev() {
                    let d = &x % &b;
                    digits[i] = d.try_into().unwrap_or(0);
                    x /= &b;
                }
                self.buf = digits;
                return;
            }
            p *= 2;
        }
    }
}

#[cfg(feature = "spigot")]
impl DigitStream for Spigot {
    fn base(&self) -> u64 {
        self.base
    }
    fn next_digit(&mut self) -> u64 {
        match self.constant {
            Constant::Pi | Constant::E | Constant::Ln2 => {
                if self.pos >= self.buf.len() {
                    let n = (self.buf.len() * 2).max(64);
                    self.refill(n);
                }
                let d = self.buf[self.pos];
                self.pos += 1;
                d
            }
            Constant::Liouville => {
                // digit at fractional position m (1-based) is 1 iff m = k!
                self.n += 1;
                let m = self.n;
                let (mut f, mut k) = (1u64, 1u64);
                while f < m {
                    k += 1;
                    f = f.saturating_mul(k);
                }
                (f == m) as u64
            }
            Constant::Champernowne => {
                if self.champ.1.is_empty() {
                    let mut x = self.champ.0;
                    let mut v = vec![];
                    while x > 0 {
                        v.push(x % self.base);
                        x /= self.base;
                    }
                    self.champ.1 = v; // reversed: pop from the end gives MSD first
                    self.champ.0 += 1;
                }
                self.champ.1.pop().unwrap()
            }
            Constant::ThueMorse => {
                let k = self.n;
                self.n += 1;
                (k.count_ones() % 2) as u64
            }
        }
    }
    fn describe(&self) -> String {
        if self.start > 0 {
            format!("spigot:{}/{}@{}", self.constant.name(), self.base, self.start)
        } else {
            format!("spigot:{}/{}", self.constant.name(), self.base)
        }
    }
}

/// A fixed digit sequence (for tests and replayed digit streams).
pub struct Digits {
    pub digits: Vec<u64>,
    pub pos: usize,
    pub base: u64,
}
impl DigitStream for Digits {
    fn base(&self) -> u64 {
        self.base
    }
    fn next_digit(&mut self) -> u64 {
        let d = *self.digits.get(self.pos).expect("digit sequence exhausted");
        self.pos += 1;
        d
    }
    fn describe(&self) -> String {
        format!("digits/{}", self.base)
    }
}

// --------------------------------------------------------------- decoding

pub const KMAX: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub index: usize,
    pub digits: u32,
    /// the deterministic fallback was used after KMAX digits
    pub fallback: bool,
}

/// Exact lazy interval decoding over positive rational weights.
pub fn interval_decode(weights: &[Q], s: &mut dyn DigitStream, kmax: u32) -> Decoded {
    assert!(!weights.is_empty());
    assert!(weights.iter().all(|w| w.is_positive()), "alternatives have positive weight");
    if weights.len() == 1 {
        return Decoded { index: 0, digits: 0, fallback: false };
    }
    let l = score_core::q::lcm_denoms(weights);
    let ints: Vec<BigUint> = weights.iter().map(|w| w.numer_big() * (&l / w.denom_big())).collect();
    let mut cum = vec![BigUint::zero()];
    for w in &ints {
        let last = cum.last().unwrap().clone();
        cum.push(last + w);
    }
    let total = cum.last().unwrap().clone();
    let b = BigUint::from(s.base());
    let mut a = BigUint::zero();
    let mut bk = BigUint::one();
    let mut k = 0u32;
    loop {
        a = a * &b + BigUint::from(s.next_digit());
        bk *= &b;
        k += 1;
        let at = &a * &total;
        let a1t = (&a + 1u32) * &total;
        // bins are ordered; find the first j with cum[j+1]*b^k > A*T
        for j in 0..weights.len() {
            if cum[j].clone() * &bk <= at && a1t <= cum[j + 1].clone() * &bk {
                return Decoded { index: j, digits: k, fallback: false };
            }
        }
        if k >= kmax {
            let j = (0..weights.len()).filter(|&i| cum[i].clone() * &bk <= at).max().unwrap_or(0);
            return Decoded { index: j, digits: k, fallback: true };
        }
    }
}

// ----------------------------------------------------------------- sources

/// What the chooser is told about a choice (all a human needs to decide).
pub struct ChoiceCtx<'a> {
    pub step: u64,
    pub stream: &'a str,
    pub onset: &'a Q,
    /// a description of each alternative, in canonical order
    pub labels: &'a [String],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub index: usize,
    pub digits: u32,
    pub fallback: bool,
}

/// A chance source (Definition 3.3 names it a parameter of the semantics).
pub trait Chance {
    fn choose(&mut self, weights: &[Q], ctx: &ChoiceCtx) -> Result<Choice, String>;
    fn spec(&self) -> String;
}

/// A digit stream with interval decoding.
pub struct DigitChance {
    pub stream: Box<dyn DigitStream>,
    pub kmax: u32,
}

impl Chance for DigitChance {
    fn choose(&mut self, weights: &[Q], _: &ChoiceCtx) -> Result<Choice, String> {
        let d = interval_decode(weights, self.stream.as_mut(), self.kmax);
        Ok(Choice { index: d.index, digits: d.digits, fallback: d.fallback })
    }
    fn spec(&self) -> String {
        self.stream.describe()
    }
}

/// Build a source from `prng:SEED` or `spigot:C/B`. (`human` and `replay:FILE`
/// need I/O and are provided by the binary.)
pub fn source(spec: &str, kmax: u32) -> Result<Box<dyn Chance>, String> {
    if let Some(seed) = spec.strip_prefix("prng:") {
        let seed: u64 = seed.parse().map_err(|_| format!("bad PRNG seed `{seed}`"))?;
        return Ok(Box::new(DigitChance { stream: Box::new(Prng::new(seed)), kmax }));
    }
    if let Some(rest) = spec.strip_prefix("spigot:") {
        #[cfg(feature = "spigot")]
        {
            // `spigot:C/B@Q` starts at driver position Q (Q digits already read,
            // e.g. by a written first note)
            let (rest, skip) = match rest.split_once('@') {
                Some((r, q)) => (r, q.parse::<u64>().map_err(|_| format!("bad driver position `{q}`"))?),
                None => (rest, 0),
            };
            let (c, b) = rest.split_once('/').ok_or("spigot sources are `spigot:CONST/BASE[@POS]`")?;
            let c = Constant::parse(c)
                .ok_or_else(|| format!("unknown constant `{c}` (pi, e, ln2, liouville, champernowne, thue-morse)"))?;
            let b: u64 = b.parse().map_err(|_| format!("bad base `{b}`"))?;
            let mut sp = Spigot::new(c, b)?;
            for _ in 0..skip {
                sp.next_digit();
            }
            sp.start = skip;
            return Ok(Box::new(DigitChance { stream: Box::new(sp), kmax }));
        }
        #[cfg(not(feature = "spigot"))]
        return Err(format!("spigot sources need the `spigot` feature (`{rest}`)"));
    }
    Err(format!("unknown chance source `{spec}` (prng:SEED, spigot:C/B, human, replay:FILE)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prng_is_deterministic() {
        let mut a = Prng::new(7);
        let mut b = Prng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        assert_ne!(Prng::new(7).next_u64(), Prng::new(8).next_u64());
    }

    #[test]
    fn decode_uniform_in_matching_base_uses_one_digit() {
        // the embedding (Prop. of drawn distributions): uniform over b bins in
        // base b reads exactly one digit, and that digit is the bin
        let w = vec![Q::ONE; 5];
        let mut d = Digits { digits: vec![3, 0, 4, 1], pos: 0, base: 5 };
        for expect in [3, 0, 4, 1] {
            let r = interval_decode(&w, &mut d, KMAX);
            assert_eq!((r.index, r.digits), (expect, 1));
        }
    }

    #[test]
    fn decode_needs_more_digits_when_straddling() {
        // bins 1/3, 2/3 in base 2: digit 0 -> [0,1/2) straddles 1/3
        let w = vec![Q::ONE, Q::int(2)];
        let mut d = Digits { digits: vec![0, 0], pos: 0, base: 2 };
        let r = interval_decode(&w, &mut d, KMAX);
        assert_eq!((r.index, r.digits), (0, 2)); // [0,1/4) inside [0,1/3)
    }

    #[cfg(feature = "spigot")]
    fn take(c: Constant, b: u64, n: usize) -> String {
        let mut s = Spigot::new(c, b).unwrap();
        (0..n).map(|_| std::char::from_digit(s.next_digit() as u32, 36).unwrap()).collect()
    }

    #[cfg(feature = "spigot")]
    #[test]
    fn spigot_digits_are_exact_fractional_digits() {
        assert_eq!(
            take(Constant::Pi, 16, 128),
            "243f6a8885a308d313198a2e03707344a4093822299f31d0082efa98ec4e6c89452821e638d01377be5466cf34e90c6cc0ac29b7c97c50dd3f84d5b5b5470917"
        );
        assert_eq!(take(Constant::Pi, 10, 50), "14159265358979323846264338327950288419716939937510");
        assert_eq!(take(Constant::Pi, 2, 10), "0010010000");
        assert_eq!(take(Constant::E, 10, 50), "71828182845904523536028747135266249775724709369995");
        assert_eq!(take(Constant::E, 2, 16), "1011011111100001");
        assert_eq!(take(Constant::Ln2, 10, 40), "6931471805599453094172321214581765680755");
        assert_eq!(take(Constant::Ln2, 2, 16), "1011000101110010");
        assert_eq!(take(Constant::Liouville, 10, 24), "110001000000000000000001");
        assert_eq!(take(Constant::Champernowne, 10, 15), "123456789101112");
        assert_eq!(take(Constant::Champernowne, 2, 9), "110111001");
        assert_eq!(take(Constant::ThueMorse, 2, 16), "0110100110010110");
        // long runs agree with a fresh generator (refills are consistent)
        let a = take(Constant::Pi, 5, 700);
        let b = take(Constant::Pi, 5, 1000);
        assert_eq!(a, b[..700]);
    }
}
