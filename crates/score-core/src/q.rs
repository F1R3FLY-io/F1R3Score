//! Exact non-negative rationals (R-exact).
//!
//! `Q` has a fast path of reduced `u64/u64` and promotes to a `BigRational`
//! on overflow. The representation is canonical: a value that fits the fast
//! path is always stored there, so derived equality and hashing are sound.

use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Q {
    /// reduced numerator / denominator, denominator > 0
    S(u64, u64),
    /// only when it does not fit `S`; always positive denominator, non-negative
    B(Box<BigRational>),
}

fn gcd(a: u64, b: u64) -> u64 {
    a.gcd(&b)
}

impl Q {
    pub const ZERO: Q = Q::S(0, 1);
    pub const ONE: Q = Q::S(1, 1);

    pub fn new(n: u64, d: u64) -> Q {
        assert!(d != 0, "zero denominator");
        if n == 0 {
            return Q::ZERO;
        }
        let g = gcd(n, d);
        Q::S(n / g, d / g)
    }
    pub fn int(n: u64) -> Q {
        Q::S(n, 1)
    }
    fn from_big(r: BigRational) -> Q {
        assert!(!r.is_negative_(), "Q is non-negative");
        let n = r.numer();
        let d = r.denom();
        if let (Some(n), Some(d)) = (n.to_u64(), d.to_u64()) {
            return Q::new(n, d);
        }
        Q::B(Box::new(r))
    }
    pub fn to_big(&self) -> BigRational {
        match self {
            Q::S(n, d) => BigRational::new(BigInt::from(*n), BigInt::from(*d)),
            Q::B(b) => (**b).clone(),
        }
    }
    pub fn is_zero(&self) -> bool {
        matches!(self, Q::S(0, _))
    }
    pub fn is_positive(&self) -> bool {
        !self.is_zero()
    }
    pub fn numer_big(&self) -> BigUint {
        match self {
            Q::S(n, _) => BigUint::from(*n),
            Q::B(b) => b.numer().to_biguint().unwrap(),
        }
    }
    pub fn denom_big(&self) -> BigUint {
        match self {
            Q::S(_, d) => BigUint::from(*d),
            Q::B(b) => b.denom().to_biguint().unwrap(),
        }
    }
    pub fn add(&self, o: &Q) -> Q {
        if let (Q::S(a, b), Q::S(c, d)) = (self, o) {
            let (a, b, c, d) = (*a as u128, *b as u128, *c as u128, *d as u128);
            let n = a * d + c * b;
            let m = b * d;
            let g = n.gcd(&m).max(1);
            let (n, m) = (n / g, m / g);
            if n <= u64::MAX as u128 && m <= u64::MAX as u128 {
                return Q::new(n as u64, m as u64);
            }
        }
        Q::from_big(self.to_big() + o.to_big())
    }
    /// Saturating subtraction (never negative); used only for diagnostics.
    pub fn sub_sat(&self, o: &Q) -> Q {
        if self <= o {
            return Q::ZERO;
        }
        Q::from_big(self.to_big() - o.to_big())
    }
    pub fn mul(&self, o: &Q) -> Q {
        if self.is_zero() || o.is_zero() {
            return Q::ZERO;
        }
        if let (Q::S(a, b), Q::S(c, d)) = (self, o) {
            let (a, b, c, d) = (*a as u128, *b as u128, *c as u128, *d as u128);
            let n = a * c;
            let m = b * d;
            let g = n.gcd(&m).max(1);
            let (n, m) = (n / g, m / g);
            if n <= u64::MAX as u128 && m <= u64::MAX as u128 {
                return Q::new(n as u64, m as u64);
            }
        }
        Q::from_big(self.to_big() * o.to_big())
    }
    pub fn div(&self, o: &Q) -> Q {
        assert!(!o.is_zero(), "division by zero");
        Q::from_big(self.to_big() / o.to_big())
    }
    pub fn max(&self, o: &Q) -> Q {
        if self >= o {
            self.clone()
        } else {
            o.clone()
        }
    }
    pub fn min(&self, o: &Q) -> Q {
        if self <= o {
            self.clone()
        } else {
            o.clone()
        }
    }
    pub fn to_f64(&self) -> f64 {
        match self {
            Q::S(n, d) => *n as f64 / *d as f64,
            Q::B(b) => b.to_f64().unwrap_or(f64::NAN),
        }
    }
    /// Parse `3`, `3/8`, `0.3`, `1.25`. Decimals are read exactly.
    pub fn parse(s: &str) -> Option<Q> {
        let s = s.trim();
        if let Some((a, b)) = s.split_once('/') {
            let n: BigUint = a.trim().parse().ok()?;
            let d: BigUint = b.trim().parse().ok()?;
            if d.is_zero() {
                return None;
            }
            return Some(Q::from_big(BigRational::new(n.into(), d.into())));
        }
        if let Some((a, b)) = s.split_once('.') {
            if b.is_empty() || !b.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let ip: BigUint = if a.is_empty() { BigUint::zero() } else { a.parse().ok()? };
            let fp: BigUint = b.parse().ok()?;
            let den = num_traits::pow(BigUint::from(10u32), b.len());
            let num = ip * &den + fp;
            return Some(Q::from_big(BigRational::new(num.into(), den.into())));
        }
        let n: BigUint = s.parse().ok()?;
        Some(Q::from_big(BigRational::new(n.into(), BigInt::one())))
    }
}

trait NegCheck {
    fn is_negative_(&self) -> bool;
}
impl NegCheck for BigRational {
    fn is_negative_(&self) -> bool {
        use num_traits::Signed;
        self.is_negative()
    }
}

impl PartialOrd for Q {
    fn partial_cmp(&self, o: &Q) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Q {
    fn cmp(&self, o: &Q) -> Ordering {
        match (self, o) {
            (Q::S(a, b), Q::S(c, d)) => ((*a as u128) * (*d as u128)).cmp(&((*c as u128) * (*b as u128))),
            _ => self.to_big().cmp(&o.to_big()),
        }
    }
}

impl fmt::Display for Q {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Q::S(n, 1) => write!(f, "{n}"),
            Q::S(n, d) => write!(f, "{n}/{d}"),
            Q::B(b) => {
                if b.denom().is_one() {
                    write!(f, "{}", b.numer())
                } else {
                    write!(f, "{}/{}", b.numer(), b.denom())
                }
            }
        }
    }
}
impl fmt::Debug for Q {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
impl Default for Q {
    fn default() -> Q {
        Q::ZERO
    }
}

/// Least common multiple of the denominators, as a big integer.
pub fn lcm_denoms(qs: &[Q]) -> BigUint {
    let mut l = BigUint::one();
    for q in qs {
        let d = q.denom_big();
        l = l.lcm(&d);
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_exact() {
        assert_eq!(Q::parse("0.3").unwrap(), Q::new(3, 10));
        assert_eq!(Q::parse("3/8").unwrap(), Q::new(3, 8));
        assert_eq!(Q::parse("6/16").unwrap(), Q::new(3, 8));
        assert_eq!(Q::parse("2").unwrap(), Q::int(2));
        assert!(Q::parse("1/0").is_none());
    }
    #[test]
    fn promotes_and_demotes() {
        let big = Q::new(u64::MAX, 1);
        let s = big.mul(&big);
        assert!(matches!(s, Q::B(_)));
        let back = s.div(&big);
        assert_eq!(back, big);
        assert!(matches!(back, Q::S(..)));
    }
    #[test]
    fn ordering() {
        assert!(Q::new(1, 3) < Q::new(1, 2));
        assert_eq!(Q::new(1, 4).add(&Q::new(1, 4)), Q::new(1, 2));
    }
}
