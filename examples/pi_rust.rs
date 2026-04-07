use rug::{Assign, Integer};
use rug::ops::AddAssignRound;
use std::time::Instant;

macro_rules! imul {
    ($a:expr, $b:expr) => { Integer::from(&$a * $b) }
}

fn main() {
    let target: usize = 100_000;

    let ref_1k = "1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679821480865132823066470938446095505822317253594081284811174502841027019385211055596446229489549303819644288109756659334461284756482337867831652712019091456485669234603486104543266482133936072602491412737245870066063155881748815209209628292540917153643678925903600113305305488204665213841469519415116094330572703657595919530921861173819326117931051185480744623799627495673518857527248912279381830119491298336733624406566430860213949463952247371907021798609437027705392171762931767523846748184676694051320005681271452635608277857713427577896091736371787214684409012249534301465495853710507922796892589235420199561121290219608640344181598136297747713099605187072113499999983729780499510597317328160963185950244594553469083026425223082533446850352619311881710100031378387528865875332083814206171776691473035982534904287554687311595628638823537875937519577818577805321712268066130019278766111959092164201989";
    let ref_50k = "06526234053394391421112718106910522900246574236041";
    let ref_100k = "70150789337728658035712790913767420805655493624646";

    println!("PI spigot (Rust/GMP) — computing {} digits", target);
    println!("==========================================");
    println!();

    let start = Instant::now();

    let mut q = Integer::from(1);
    let mut r = Integer::from(0);
    let mut s = Integer::from(0);
    let mut t = Integer::from(1);
    let mut k: u64 = 1;
    let mut digits: usize = 0;
    let mut errors: usize = 0;
    let mut buf = String::with_capacity(64);

    while digits <= target {
        // extract: floor((q*3 + r) / (s*3 + t))
        let num3 = Integer::from(&q * 3u32) + &r;
        let den3 = Integer::from(&s * 3u32) + &t;
        let d_val = Integer::from(&num3 / &den3).to_u32().unwrap_or(0) as u8;

        // safe: floor((q*4 + r) / (s*4 + t))
        let num4 = Integer::from(&q * 4u32) + &r;
        let den4 = Integer::from(&s * 4u32) + &t;
        let d4_val = Integer::from(&num4 / &den4).to_u32().unwrap_or(0) as u8;

        if d_val == d4_val {
            let dint = Integer::from(d_val as u32);
            if digits == 0 {
                digits = 1;
            } else {
                let pos = digits - 1;
                if pos < ref_1k.len() {
                    if d_val != ref_1k.as_bytes()[pos] - b'0' {
                        errors += 1;
                    }
                }
                buf.push((b'0' + d_val) as char);
                if buf.len() > 50 {
                    buf = buf[buf.len() - 50..].to_string();
                }
                digits += 1;

                let count = digits - 1;
                let ms = start.elapsed().as_millis() as u64;

                if count == 1000 {
                    if errors == 0 {
                        println!("  [1,000] ALL 1000 digits match reference  ({}ms)", ms);
                    } else {
                        println!("  [1,000] {} mismatches!  ({}ms)", errors, ms);
                    }
                }
                if count == 50000 {
                    let rate = if ms > 0 { count as u64 * 1000 / ms } else { 0 };
                    if buf == ref_50k { println!("  [50,000] PASS  ({}ms, ~{} d/s)", ms, rate); }
                    else { println!("  [50,000] MISMATCH!  ({}ms)", ms); }
                    println!("    ...{}", buf);
                }
                if count == 100000 {
                    let rate = if ms > 0 { count as u64 * 1000 / ms } else { 0 };
                    if buf == ref_100k { println!("  [100,000] PASS  ({}ms, ~{} d/s)", ms, rate); }
                    else { println!("  [100,000] MISMATCH!  ({}ms)", ms); }
                    println!("    ...{}", buf);
                }
                if count % 10000 == 0 && count != 50000 && count != 100000 {
                    let rate = if ms > 0 { count as u64 * 1000 / ms } else { 0 };
                    println!("  [{}] ({}ms, ~{} d/s)", count, ms, rate);
                }
            }
            // consume: q = 10*(q - d*s), r = 10*(r - d*t)
            q -= Integer::from(&dint * &s);
            q *= 10;
            r -= Integer::from(&dint * &t);
            r *= 10;
        } else {
            // absorb: compose with (k, 4k+2, 0, 2k+1)
            let u = 4 * k + 2;
            let v = 2 * k + 1;
            let new_r = Integer::from(&q * u) + Integer::from(&r * v);
            let new_t = Integer::from(&s * u) + Integer::from(&t * v);
            q *= k;
            s *= k;
            r = new_r;
            t = new_t;
            k += 1;
        }
    }

    let total = start.elapsed();
    println!();
    println!("Done: {} digits of PI in {}ms", digits - 1, total.as_millis());
    if errors == 0 { println!("ALL checkpoints PASSED (1K full + 50K + 100K)"); }
    else { println!("TOTAL MISMATCHES: {}", errors); }
}
