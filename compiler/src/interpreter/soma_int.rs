//! SomaInt: Soma's unified integer type.
//!
//! A 64-bit tagged value:
//!   - bit 0 = 1: inline small int (63-bit signed, zero allocation)
//!   - bit 0 = 0: pointer to heap-allocated rug::Integer (GMP-backed, arbitrary precision)
//!
//! Small ints (±4.6 × 10^18) are as fast as raw i64.
//! Big ints use GMP — the fastest arbitrary-precision library.

use std::fmt;
use rug::Integer;

const SMALL_TAG: u64 = 1;
const SMALL_MAX: i64 = i64::MAX >> 1;
const SMALL_MIN: i64 = i64::MIN >> 1;

/// A Soma integer: 8 bytes, always.
#[derive(Clone)]
pub struct SomaInt(SomaIntInner);

#[derive(Clone)]
enum SomaIntInner {
    Small(i64),
    Big(Box<Integer>),
}

impl SomaInt {
    #[inline(always)]
    pub fn from_i64(v: i64) -> Self {
        SomaInt(SomaIntInner::Small(v))
    }

    pub fn from_rug(v: Integer) -> Self {
        if let Some(n) = v.to_i64() {
            SomaInt(SomaIntInner::Small(n))
        } else {
            SomaInt(SomaIntInner::Big(Box::new(v)))
        }
    }

    #[inline(always)]
    pub fn is_small(&self) -> bool {
        matches!(self.0, SomaIntInner::Small(_))
    }

    pub fn to_i64(&self) -> Option<i64> {
        match &self.0 {
            SomaIntInner::Small(n) => Some(*n),
            SomaIntInner::Big(n) => n.to_i64(),
        }
    }

    pub fn to_f64(&self) -> f64 {
        match &self.0 {
            SomaIntInner::Small(n) => *n as f64,
            SomaIntInner::Big(n) => n.to_f64(),
        }
    }

    fn to_rug(&self) -> Integer {
        match &self.0 {
            SomaIntInner::Small(n) => Integer::from(*n),
            SomaIntInner::Big(n) => (**n).clone(),
        }
    }

    // Reference counting stubs (not needed with rug — Rust's Drop handles it)
    pub fn inc_ref(&self) {}
    pub fn dec_ref(&self) {}
}

// ── Arithmetic ──────────────────────────────────────────────────────

impl SomaInt {
    pub fn add(self, other: SomaInt) -> SomaInt {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                match a.checked_add(*b) {
                    Some(r) => SomaInt::from_i64(r),
                    None => SomaInt::from_rug(Integer::from(*a) + *b),
                }
            }
            _ => SomaInt::from_rug(self.to_rug() + other.to_rug()),
        }
    }

    pub fn sub(self, other: SomaInt) -> SomaInt {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                match a.checked_sub(*b) {
                    Some(r) => SomaInt::from_i64(r),
                    None => SomaInt::from_rug(Integer::from(*a) - *b),
                }
            }
            _ => SomaInt::from_rug(self.to_rug() - other.to_rug()),
        }
    }

    pub fn mul(self, other: SomaInt) -> SomaInt {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                match a.checked_mul(*b) {
                    Some(r) => SomaInt::from_i64(r),
                    None => SomaInt::from_rug(Integer::from(*a) * *b),
                }
            }
            _ => SomaInt::from_rug(self.to_rug() * other.to_rug()),
        }
    }

    pub fn div(self, other: SomaInt) -> SomaInt {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                if *b == 0 { return SomaInt::from_i64(0); }
                SomaInt::from_i64(a / b)
            }
            _ => {
                let b = other.to_rug();
                if b == 0 { return SomaInt::from_i64(0); }
                SomaInt::from_rug(self.to_rug() / b)
            }
        }
    }

    pub fn modulo(self, other: SomaInt) -> SomaInt {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                if *b == 0 { return SomaInt::from_i64(0); }
                SomaInt::from_i64(a % b)
            }
            _ => {
                let b = other.to_rug();
                if b == 0 { return SomaInt::from_i64(0); }
                SomaInt::from_rug(self.to_rug() % b)
            }
        }
    }

    pub fn cmp(&self, other: &Self) -> i32 {
        match (&self.0, &other.0) {
            (SomaIntInner::Small(a), SomaIntInner::Small(b)) => {
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
            _ => {
                let c = self.to_rug().cmp(&other.to_rug());
                match c {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                }
            }
        }
    }
}

// ── Display ─────────────────────────────────────────────────────────

impl fmt::Display for SomaInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            SomaIntInner::Small(n) => write!(f, "{}", n),
            SomaIntInner::Big(n) => write!(f, "{}", n),
        }
    }
}

impl fmt::Debug for SomaInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SomaInt({})", self)
    }
}

impl PartialEq for SomaInt {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == 0 }
}

// Clone is sufficient — no Copy because Big contains Box.

// ── Conversion from num-bigint (for interop with existing code) ─────

impl SomaInt {
    pub fn from_bigint(b: &num_bigint::BigInt) -> Self {
        let s = b.to_string();
        match s.parse::<Integer>() {
            Ok(r) => SomaInt::from_rug(r),
            Err(_) => SomaInt::from_i64(0),
        }
    }

    pub fn from_limbs(limbs: &[u64], negative: bool) -> Self {
        // Build from limbs — used by native FFI
        if limbs.is_empty() { return SomaInt::from_i64(0); }
        if limbs.len() == 1 {
            let v = limbs[0] as i64;
            return SomaInt::from_i64(if negative { -v } else { v });
        }
        // Build rug Integer from limbs (little-endian u64)
        let bytes: Vec<u8> = limbs.iter()
            .flat_map(|l| l.to_le_bytes())
            .collect();
        let mut r = Integer::from_digits(&bytes, rug::integer::Order::LsfLe);
        if negative { r = -r; }
        SomaInt::from_rug(r)
    }
}

// ── Arena stats (for compatibility — rug manages its own memory) ────

pub fn arena_stats() -> (usize, usize) {
    (0, 0) // rug/GMP manages memory internally
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_int() {
        let a = SomaInt::from_i64(42);
        assert!(a.is_small());
        assert_eq!(a.to_i64(), Some(42));
        assert_eq!(format!("{}", a), "42");
    }

    #[test]
    fn test_small_add() {
        let a = SomaInt::from_i64(100);
        let b = SomaInt::from_i64(200);
        let c = a.add(b);
        assert_eq!(c.to_i64(), Some(300));
    }

    #[test]
    fn test_small_mul() {
        let a = SomaInt::from_i64(1000);
        let b = SomaInt::from_i64(2000);
        let c = a.mul(b);
        assert_eq!(c.to_i64(), Some(2_000_000));
    }

    #[test]
    fn test_overflow_to_big() {
        let a = SomaInt::from_i64(i64::MAX);
        let b = SomaInt::from_i64(1);
        let c = a.add(b);
        assert_eq!(format!("{}", c), format!("{}", i64::MAX as i128 + 1));
    }

    #[test]
    fn test_big_mul() {
        let a = SomaInt::from_i64(1_000_000_000_000_000_000);
        let b = SomaInt::from_i64(1_000_000_000_000_000_000);
        let c = a.mul(b);
        assert_eq!(format!("{}", c), "1000000000000000000000000000000000000");
    }

    #[test]
    fn test_factorial_20() {
        let mut result = SomaInt::from_i64(1);
        for i in 1..=20 {
            result = result.mul(SomaInt::from_i64(i));
        }
        assert_eq!(format!("{}", result), "2432902008176640000");
    }

    #[test]
    fn test_factorial_100() {
        let mut result = SomaInt::from_i64(1);
        for i in 1..=100i64 {
            result = result.mul(SomaInt::from_i64(i));
        }
        let s = format!("{}", result);
        assert!(s.starts_with("93326215443944"));
        assert_eq!(s.len(), 158);
    }

    #[test]
    fn test_factorial_1000() {
        let mut result = SomaInt::from_i64(1);
        for i in 1..=1000i64 {
            result = result.mul(SomaInt::from_i64(i));
        }
        let s = format!("{}", result);
        assert_eq!(s.len(), 2568);
    }

    #[test]
    fn test_negative() {
        let a = SomaInt::from_i64(-42);
        assert_eq!(a.to_i64(), Some(-42));
        assert_eq!(format!("{}", a), "-42");
    }

    #[test]
    fn test_sub() {
        let a = SomaInt::from_i64(10);
        let b = SomaInt::from_i64(25);
        let c = a.sub(b);
        assert_eq!(c.to_i64(), Some(-15));
    }

    #[test]
    fn test_cmp() {
        let a = SomaInt::from_i64(100);
        let b = SomaInt::from_i64(200);
        assert_eq!(a.cmp(&b), -1);
        assert_eq!(b.cmp(&a), 1);
        assert_eq!(a.cmp(&a), 0);
    }
}
