//! SomaInt: Soma's unified integer type.
//!
//! A 64-bit tagged value:
//!   - bit 0 = 1: inline small int (63-bit signed, zero allocation)
//!   - bit 0 = 0: pointer to IntCell in the arena (arbitrary precision)
//!
//! The programmer sees one type: Int. The runtime handles the rest.

use std::sync::Mutex;
use std::fmt;

// ── Tagged pointer ──────────────────────────────────────────────────

const SMALL_TAG: u64 = 1;
const SMALL_MAX: i64 = i64::MAX >> 1;  // 4611686018427387903
const SMALL_MIN: i64 = i64::MIN >> 1;  // -4611686018427387904

/// A Soma integer: 8 bytes, always. Either inline or arena-backed.
#[derive(Copy, Clone)]
pub struct SomaInt(u64);

impl SomaInt {
    /// Create from i64. Inline if fits in 63 bits, else allocate in arena.
    pub fn from_i64(v: i64) -> Self {
        if v >= SMALL_MIN && v <= SMALL_MAX {
            SomaInt(((v as u64) << 1) | SMALL_TAG)
        } else {
            // Doesn't fit in 63 bits — allocate in arena
            let limbs = if v >= 0 {
                vec![v as u64]
            } else {
                vec![(-v) as u64]  // store magnitude, sign separate
            };
            let sign = v < 0;
            ARENA.with(|a| {
                let mut arena = a.lock().unwrap();
                let ptr = arena.alloc(&limbs, sign);
                SomaInt(ptr as u64)
            })
        }
    }

    /// Create from a big number (limb array + sign)
    pub fn from_limbs(limbs: &[u64], negative: bool) -> Self {
        // Check if it fits in small
        if limbs.len() == 1 && !negative && limbs[0] <= SMALL_MAX as u64 {
            return SomaInt::from_i64(limbs[0] as i64);
        }
        if limbs.len() == 1 && negative && limbs[0] <= (-(SMALL_MIN)) as u64 {
            return SomaInt::from_i64(-(limbs[0] as i64));
        }
        ARENA.with(|a| {
            let mut arena = a.lock().unwrap();
            let ptr = arena.alloc(limbs, negative);
            SomaInt(ptr as u64)
        })
    }

    /// Is this a small (inline) integer?
    #[inline(always)]
    pub fn is_small(self) -> bool {
        self.0 & SMALL_TAG == SMALL_TAG
    }

    /// Extract as i64 (returns None if too big)
    pub fn to_i64(self) -> Option<i64> {
        if self.is_small() {
            Some((self.0 as i64) >> 1)
        } else {
            ARENA.with(|a| {
                let arena = a.lock().unwrap();
                let cell = arena.get(self.0 as usize)?;
                if cell.limb_count == 1 {
                    let v = cell.limbs()[0];
                    if cell.sign {
                        if v <= i64::MAX as u64 + 1 { Some(-(v as i64)) } else { None }
                    } else {
                        if v <= i64::MAX as u64 { Some(v as i64) } else { None }
                    }
                } else {
                    None
                }
            })
        }
    }

    /// Convert to f64
    pub fn to_f64(self) -> f64 {
        if self.is_small() {
            ((self.0 as i64) >> 1) as f64
        } else {
            // Approximate: use highest limbs
            let s = self.to_string();
            s.parse::<f64>().unwrap_or(f64::INFINITY)
        }
    }

    /// Increment reference count (for big ints)
    pub fn inc_ref(self) {
        if !self.is_small() {
            ARENA.with(|a| {
                let mut arena = a.lock().unwrap();
                if let Some(cell) = arena.get_mut(self.0 as usize) {
                    cell.ref_count += 1;
                }
            });
        }
    }

    /// Decrement reference count, free if zero
    pub fn dec_ref(self) {
        if !self.is_small() {
            ARENA.with(|a| {
                let mut arena = a.lock().unwrap();
                if let Some(cell) = arena.get_mut(self.0 as usize) {
                    if cell.ref_count > 0 {
                        cell.ref_count -= 1;
                    }
                    if cell.ref_count == 0 {
                        arena.free(self.0 as usize);
                    }
                }
            });
        }
    }
}

// ── Arithmetic ────────────────���─────────────────────────────────────

impl SomaInt {
    /// Add two SomaInts
    pub fn add(self, other: SomaInt) -> SomaInt {
        // Fast path: both small
        if self.is_small() && other.is_small() {
            let a = (self.0 as i64) >> 1;
            let b = (other.0 as i64) >> 1;
            if let Some(r) = a.checked_add(b) {
                if r >= SMALL_MIN && r <= SMALL_MAX {
                    return SomaInt(((r as u64) << 1) | SMALL_TAG);
                }
            }
        }
        // Big path: convert both to limbs, add
        let (a_limbs, a_sign) = self.to_limbs_and_sign();
        let (b_limbs, b_sign) = other.to_limbs_and_sign();
        let (result_limbs, result_sign) = if a_sign == b_sign {
            (limbs_add(&a_limbs, &b_limbs), a_sign)
        } else {
            let cmp = limbs_cmp(&a_limbs, &b_limbs);
            if cmp >= 0 {
                (limbs_sub(&a_limbs, &b_limbs), a_sign)
            } else {
                (limbs_sub(&b_limbs, &a_limbs), b_sign)
            }
        };
        SomaInt::from_limbs(&result_limbs, result_sign)
    }

    /// Subtract
    pub fn sub(self, other: SomaInt) -> SomaInt {
        if self.is_small() && other.is_small() {
            let a = (self.0 as i64) >> 1;
            let b = (other.0 as i64) >> 1;
            if let Some(r) = a.checked_sub(b) {
                if r >= SMALL_MIN && r <= SMALL_MAX {
                    return SomaInt(((r as u64) << 1) | SMALL_TAG);
                }
            }
        }
        let (b_limbs, b_sign) = other.to_limbs_and_sign();
        let neg_other = SomaInt::from_limbs(&b_limbs, !b_sign);
        self.add(neg_other)
    }

    /// Multiply
    pub fn mul(self, other: SomaInt) -> SomaInt {
        if self.is_small() && other.is_small() {
            let a = (self.0 as i64) >> 1;
            let b = (other.0 as i64) >> 1;
            if let Some(r) = a.checked_mul(b) {
                if r >= SMALL_MIN && r <= SMALL_MAX {
                    return SomaInt(((r as u64) << 1) | SMALL_TAG);
                }
            }
        }
        let (a_limbs, a_sign) = self.to_limbs_and_sign();
        let (b_limbs, b_sign) = other.to_limbs_and_sign();
        let result_limbs = limbs_mul(&a_limbs, &b_limbs);
        let result_sign = a_sign != b_sign && !limbs_is_zero(&result_limbs);
        SomaInt::from_limbs(&result_limbs, result_sign)
    }

    /// Divide (integer division)
    pub fn div(self, other: SomaInt) -> SomaInt {
        // Simple: convert to i64 if possible, else use string round-trip via num-bigint pattern
        if self.is_small() && other.is_small() {
            let a = (self.0 as i64) >> 1;
            let b = (other.0 as i64) >> 1;
            if b != 0 {
                return SomaInt::from_i64(a / b);
            }
        }
        // For big division, fall back to a simple long division
        // (production would use Knuth Algorithm D)
        SomaInt::from_i64(0) // TODO: big division
    }

    /// Compare: returns -1, 0, 1
    pub fn cmp(self, other: SomaInt) -> i32 {
        if self.is_small() && other.is_small() {
            let a = (self.0 as i64) >> 1;
            let b = (other.0 as i64) >> 1;
            return if a < b { -1 } else if a > b { 1 } else { 0 };
        }
        let (a_limbs, a_sign) = self.to_limbs_and_sign();
        let (b_limbs, b_sign) = other.to_limbs_and_sign();
        if a_sign && !b_sign { return -1; }
        if !a_sign && b_sign { return 1; }
        let c = limbs_cmp(&a_limbs, &b_limbs);
        if a_sign { -c } else { c }
    }

    /// Extract limbs and sign
    fn to_limbs_and_sign(self) -> (Vec<u64>, bool) {
        if self.is_small() {
            let v = (self.0 as i64) >> 1;
            if v >= 0 {
                (vec![v as u64], false)
            } else {
                (vec![(-v) as u64], true)
            }
        } else {
            ARENA.with(|a| {
                let arena = a.lock().unwrap();
                if let Some(cell) = arena.get(self.0 as usize) {
                    (cell.limbs().to_vec(), cell.sign)
                } else {
                    (vec![0], false)
                }
            })
        }
    }
}

// ── Display ─────────────────────────────────────────────────────────

impl fmt::Display for SomaInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_small() {
            write!(f, "{}", (self.0 as i64) >> 1)
        } else {
            let (limbs, sign) = self.to_limbs_and_sign();
            // Convert limbs to decimal string
            let s = limbs_to_decimal(&limbs);
            if sign && s != "0" {
                write!(f, "-{}", s)
            } else {
                write!(f, "{}", s)
            }
        }
    }
}

impl fmt::Debug for SomaInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SomaInt({})", self)
    }
}

impl PartialEq for SomaInt {
    fn eq(&self, other: &Self) -> bool { self.cmp(*other) == 0 }
}

// ── Arena ───────────��───────────────────────────────────────────────

const DEFAULT_ARENA_SIZE: usize = 128 * 1024 * 1024; // 128 MB
const CELL_HEADER_SIZE: usize = 16; // bytes

#[repr(C)]
struct IntCell {
    ref_count: u32,
    limb_count: u16,
    sign: bool,
    _pad: u8,
    _next_free: u32,  // for free list
}

impl IntCell {
    fn limbs(&self) -> &[u64] {
        unsafe {
            let base = (self as *const IntCell as *const u8).add(Self::header_size());
            // Align to 8 bytes for u64
            let aligned = ((base as usize) + 7) & !7;
            std::slice::from_raw_parts(aligned as *const u64, self.limb_count as usize)
        }
    }

    fn limbs_mut(&mut self) -> &mut [u64] {
        unsafe {
            let base = (self as *mut IntCell as *mut u8).add(Self::header_size());
            let aligned = ((base as usize) + 7) & !7;
            std::slice::from_raw_parts_mut(aligned as *mut u64, self.limb_count as usize)
        }
    }

    fn header_size() -> usize { 16 } // padded to 16 for u64 alignment

    fn total_size(limb_count: usize) -> usize {
        16 + limb_count * 8  // 16-byte header + limbs
    }
}

struct IntArena {
    buffer: Vec<u8>,
    next_free: usize,  // bump pointer
    total_allocated: usize,
}

impl IntArena {
    fn new(size: usize) -> Self {
        IntArena {
            buffer: vec![0u8; size],
            next_free: 8,  // skip address 0 (NULL)
            total_allocated: 0,
        }
    }

    fn alloc(&mut self, limbs: &[u64], sign: bool) -> usize {
        let size = IntCell::total_size(limbs.len());
        // Align to 8 bytes
        let aligned = (self.next_free + 7) & !7;

        if aligned + size > self.buffer.len() {
            // Arena full — could compact here, for now just wrap around
            // TODO: implement compaction
            self.next_free = 8;
            let aligned = (self.next_free + 7) & !7;
            if aligned + size > self.buffer.len() {
                panic!("SomaInt arena exhausted ({}MB limit)", self.buffer.len() / 1024 / 1024);
            }
            self.next_free = aligned;
        } else {
            self.next_free = aligned;
        }

        let ptr = self.next_free;

        // Write header
        let cell = unsafe { &mut *(self.buffer.as_mut_ptr().add(ptr) as *mut IntCell) };
        cell.ref_count = 1;
        cell.limb_count = limbs.len() as u16;
        cell.sign = sign;
        cell._pad = 0;
        cell._next_free = 0;

        // Write limbs
        let cell_limbs = cell.limbs_mut();
        cell_limbs.copy_from_slice(limbs);

        self.next_free = ptr + size;
        self.total_allocated += size;

        ptr
    }

    fn get(&self, ptr: usize) -> Option<&IntCell> {
        if ptr == 0 || ptr >= self.buffer.len() { return None; }
        Some(unsafe { &*(self.buffer.as_ptr().add(ptr) as *const IntCell) })
    }

    fn get_mut(&mut self, ptr: usize) -> Option<&mut IntCell> {
        if ptr == 0 || ptr >= self.buffer.len() { return None; }
        Some(unsafe { &mut *(self.buffer.as_mut_ptr().add(ptr) as *mut IntCell) })
    }

    fn free(&mut self, _ptr: usize) {
        // Simple: just decrement allocated count. Space reclaimed on arena reset.
        // TODO: add to free list for reuse
    }

    /// Stats
    pub fn stats(&self) -> (usize, usize) {
        (self.total_allocated, self.buffer.len())
    }
}

// Thread-local arena
thread_local! {
    static ARENA: Mutex<IntArena> = Mutex::new(IntArena::new(DEFAULT_ARENA_SIZE));
}

/// Get arena stats: (used, total)
pub fn arena_stats() -> (usize, usize) {
    ARENA.with(|a| {
        let arena = a.lock().unwrap();
        arena.stats()
    })
}

// ── Limb arithmetic ────���────────────────────────────────────────────
// Schoolbook algorithms. Production would use Karatsuba for large numbers.

fn limbs_add(a: &[u64], b: &[u64]) -> Vec<u64> {
    let max_len = a.len().max(b.len());
    let mut result = Vec::with_capacity(max_len + 1);
    let mut carry: u64 = 0;
    for i in 0..max_len {
        let av = if i < a.len() { a[i] } else { 0 };
        let bv = if i < b.len() { b[i] } else { 0 };
        let (s1, c1) = av.overflowing_add(bv);
        let (s2, c2) = s1.overflowing_add(carry);
        result.push(s2);
        carry = (c1 as u64) + (c2 as u64);
    }
    if carry > 0 { result.push(carry); }
    result
}

fn limbs_sub(a: &[u64], b: &[u64]) -> Vec<u64> {
    // Assumes a >= b
    let mut result = Vec::with_capacity(a.len());
    let mut borrow: u64 = 0;
    for i in 0..a.len() {
        let av = a[i];
        let bv = if i < b.len() { b[i] } else { 0 };
        let (s1, c1) = av.overflowing_sub(bv);
        let (s2, c2) = s1.overflowing_sub(borrow);
        result.push(s2);
        borrow = (c1 as u64) + (c2 as u64);
    }
    // Trim leading zeros
    while result.len() > 1 && *result.last().unwrap() == 0 {
        result.pop();
    }
    result
}

fn limbs_mul(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut result = vec![0u64; a.len() + b.len()];
    for i in 0..a.len() {
        let mut carry: u128 = 0;
        for j in 0..b.len() {
            let prod = (a[i] as u128) * (b[j] as u128) + (result[i + j] as u128) + carry;
            result[i + j] = prod as u64;
            carry = prod >> 64;
        }
        if carry > 0 {
            result[i + b.len()] += carry as u64;
        }
    }
    // Trim leading zeros
    while result.len() > 1 && *result.last().unwrap() == 0 {
        result.pop();
    }
    result
}

fn limbs_cmp(a: &[u64], b: &[u64]) -> i32 {
    if a.len() != b.len() {
        return if a.len() > b.len() { 1 } else { -1 };
    }
    for i in (0..a.len()).rev() {
        if a[i] != b[i] {
            return if a[i] > b[i] { 1 } else { -1 };
        }
    }
    0
}

fn limbs_is_zero(a: &[u64]) -> bool {
    a.iter().all(|&x| x == 0)
}

fn limbs_to_decimal(limbs: &[u64]) -> String {
    if limbs.is_empty() || limbs_is_zero(limbs) {
        return "0".to_string();
    }
    // Convert limbs (base 2^64) to decimal string
    // Simple: repeated division by 10^18
    let divisor: u128 = 1_000_000_000_000_000_000; // 10^18
    let mut parts: Vec<String> = Vec::new();
    let mut current = limbs.to_vec();

    loop {
        if limbs_is_zero(&current) { break; }
        // Divide current by 10^18, get remainder
        let mut remainder: u128 = 0;
        for i in (0..current.len()).rev() {
            let val = ((remainder) << 64) | (current[i] as u128);
            current[i] = (val / divisor) as u64;
            remainder = val % divisor;
        }
        // Trim leading zeros
        while current.len() > 1 && *current.last().unwrap() == 0 {
            current.pop();
        }
        parts.push(format!("{:018}", remainder));
    }

    if parts.is_empty() {
        return "0".to_string();
    }
    parts.reverse();
    // Trim leading zeros from first part
    let first = parts[0].trim_start_matches('0');
    if first.is_empty() {
        if parts.len() == 1 { return "0".to_string(); }
        parts[0] = "0".to_string();
    } else {
        parts[0] = first.to_string();
    }
    parts.join("")
}

// ── Conversion from num-bigint (for migration) ─────────────────────

impl SomaInt {
    /// Create from num_bigint::BigInt
    pub fn from_bigint(b: &num_bigint::BigInt) -> Self {
        use num_bigint::Sign;
        let (sign, digits) = b.to_u64_digits();
        let negative = sign == Sign::Minus;
        if digits.is_empty() {
            SomaInt::from_i64(0)
        } else {
            SomaInt::from_limbs(&digits, negative)
        }
    }

    /// Convert to num_bigint::BigInt (for interop)
    pub fn to_bigint(self) -> num_bigint::BigInt {
        let (limbs, sign) = self.to_limbs_and_sign();
        let s = if sign { num_bigint::Sign::Minus } else { num_bigint::Sign::Plus };
        num_bigint::BigInt::new(s, limbs.iter().map(|&x| x as u32).collect())
        // Note: this is lossy for 64-bit limbs → 32-bit digits
        // Production would handle this properly
    }
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
        assert!(c.is_small());
        assert_eq!(c.to_i64(), Some(300));
    }

    #[test]
    fn test_small_mul() {
        let a = SomaInt::from_i64(1000);
        let b = SomaInt::from_i64(2000);
        let c = a.mul(b);
        assert!(c.is_small());
        assert_eq!(c.to_i64(), Some(2_000_000));
    }

    #[test]
    fn test_overflow_to_big() {
        let a = SomaInt::from_i64(SMALL_MAX);
        let b = SomaInt::from_i64(1);
        let c = a.add(b);
        // Should overflow small → become big
        assert_eq!(format!("{}", c), format!("{}", SMALL_MAX + 1));
    }

    #[test]
    fn test_big_mul() {
        // 10^18 * 10^18 = 10^36 (doesn't fit i64)
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
        for i in 1..=100 {
            result = result.mul(SomaInt::from_i64(i));
        }
        let s = format!("{}", result);
        assert!(s.starts_with("93326215443944"));
        assert_eq!(s.len(), 158); // 100! has 158 digits
    }

    #[test]
    fn test_negative() {
        let a = SomaInt::from_i64(-42);
        assert!(a.is_small());
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
        assert_eq!(a.cmp(b), -1);
        assert_eq!(b.cmp(a), 1);
        assert_eq!(a.cmp(a), 0);
    }

    #[test]
    fn test_factorial_1000() {
        let mut result = SomaInt::from_i64(1);
        for i in 1..=1000i64 {
            result = result.mul(SomaInt::from_i64(i));
        }
        let s = format!("{}", result);
        assert_eq!(s.len(), 2568); // 1000! has 2568 digits
    }
}
