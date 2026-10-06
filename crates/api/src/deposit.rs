//! Fixed and recurring deposits: what they are worth now and at maturity. Shared so the UI can preview one
//! before it is saved. Banks compound these quarterly, so that is what this does; the figure is an estimate and
//! the bank's statement wins.

use serde::{Deserialize, Serialize};

use crate::loan::parse_ym;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepositCalc {
    /// Put in so far: the lump sum, or the instalments paid.
    pub invested: i64,
    /// Estimated worth today, interest included.
    pub value: i64,
    /// Interest earned so far.
    pub interest: i64,
    /// Worth when it matures.
    pub maturity_value: i64,
    /// `YYYY-MM` it matures.
    pub matures: String,
    pub months_left: u32,
    pub tenure: u32,
}

/// What one unit put in `months` ago has grown to, compounding four times a year.
fn grow(rate_pct: f64, months: f64) -> f64 {
    (1.0 + rate_pct / 400.0).powf(months / 3.0)
}

/// `fixed_lump` is the deposit of a fixed deposit; `monthly` is the instalment of a recurring one (the first
/// leaves in the start month). `start` is `YYYY-MM`, `today` is `(year, month)`.
pub fn compute(fixed_lump: i64, monthly: i64, rate_pct: f64, tenure: u32, start: &str, today: (i32, u32)) -> Option<DepositCalc> {
    if tenure == 0 || rate_pct < 0.0 || (fixed_lump <= 0 && monthly <= 0) {
        return None;
    }
    let (sy, sm) = parse_ym(start)?;
    // whole months since the start month, kept between "not started" and "matured"
    let elapsed = ((today.0 - sy) * 12 + (today.1 as i32 - sm as i32)).clamp(-1, tenure as i32);
    // (put in, worth) after `months` whole months. Instalment j (from 0) leaves at the start of month j and
    // has earned for the rest; before the start month only a lump sum exists, and it has earned nothing.
    let at = |months: i32| -> (f64, f64) {
        let lump = fixed_lump as f64;
        if months < 0 {
            return (lump, lump);
        }
        let (mut put, mut worth) = (lump, lump * grow(rate_pct, months as f64));
        for j in 0..(if monthly > 0 { (months + 1).min(tenure as i32) } else { 0 }) {
            put += monthly as f64;
            worth += monthly as f64 * grow(rate_pct, (months - j) as f64);
        }
        (put, worth)
    };
    let (put, worth) = at(elapsed);
    // an instalment deposit matures a month after its last instalment, which therefore earns that month
    let (_, at_end) = at(tenure as i32);
    let end_idx = (sm as i32 - 1) + tenure as i32;
    Some(DepositCalc {
        invested: put.round() as i64,
        value: worth.round() as i64,
        interest: (worth - put).round() as i64,
        maturity_value: at_end.round() as i64,
        matures: format!("{:04}-{:02}", sy + end_idx.div_euclid(12), end_idx.rem_euclid(12) + 1),
        months_left: (tenure as i32 - elapsed.max(0)).max(0) as u32,
        tenure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_deposit_grows_quarterly() {
        // 1,00,000 at 7.2% for a year, looked at the day it matures
        let c = compute(100_000_00, 0, 7.2, 12, "2025-10", (2026, 10)).unwrap();
        assert_eq!(c.matures, "2026-10");
        assert_eq!(c.months_left, 0);
        // 1.018^4 = 1.07399
        assert!((c.value - 107_399_00).abs() < 100_00, "value {}", c.value);
        assert_eq!(c.value, c.maturity_value);
        assert_eq!(c.invested, 100_000_00);
    }

    #[test]
    fn a_recurring_deposit_counts_instalments() {
        // 5,000 a month for 12 months from 2026-01, looked at in 2026-03: three instalments so far
        let c = compute(0, 5_000_00, 7.0, 12, "2026-01", (2026, 3)).unwrap();
        assert_eq!(c.invested, 15_000_00);
        assert!(c.value > c.invested && c.value < 15_200_00);
        assert_eq!(c.months_left, 10);
        assert_eq!(c.matures, "2027-01");
        assert!(c.maturity_value > 60_000_00 && c.maturity_value < 63_000_00, "{}", c.maturity_value);
    }

    #[test]
    fn not_started_and_bad_input() {
        let c = compute(50_000_00, 0, 6.0, 24, "2027-01", (2026, 10)).unwrap();
        assert_eq!((c.invested, c.value), (50_000_00, 50_000_00));
        assert_eq!(c.months_left, 24);
        assert!(compute(0, 0, 6.0, 12, "2026-01", (2026, 10)).is_none());
        assert!(compute(1000, 0, 6.0, 0, "2026-01", (2026, 10)).is_none());
        assert!(compute(1000, 0, 6.0, 12, "nope", (2026, 10)).is_none());
    }
}
