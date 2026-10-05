//! Loan schedule maths, shared so the UI can preview a loan before it is saved.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoanCalc {
    /// Monthly instalment, minor units.
    pub emi: i64,
    pub paid: u32,
    pub left: u32,
    pub tenure: u32,
    /// Principal still owed, minor units.
    pub balance: i64,
    /// `YYYY-MM` of the last instalment.
    pub end: String,
    /// Total interest over the whole tenure, minor units.
    pub interest: i64,
}

/// `start` is `YYYY-MM` (first instalment month); `today` is `(year, month)`. `emi` overrides the computed
/// instalment when the lender's figure is known. The rate is the annual percentage.
pub fn compute(total: i64, rate_pct: f64, tenure: u32, start: &str, emi: Option<i64>, today: (i32, u32)) -> Option<LoanCalc> {
    if total <= 0 || tenure == 0 {
        return None;
    }
    let (sy, sm) = parse_ym(start)?;
    let p = total as f64;
    let r = rate_pct / 1200.0;
    let n = tenure as f64;
    let pow = |k: f64| (1.0 + r).powf(k);
    let emi_f = match emi {
        Some(e) if e > 0 => e as f64,
        _ if r > 0.0 => p * r / (1.0 - 1.0 / pow(n)),
        _ => p / n,
    };
    let months = (today.0 - sy) * 12 + (today.1 as i32 - sm as i32) + 1;
    let paid = months.clamp(0, tenure as i32) as u32;
    let k = paid as f64;
    let bal = if r > 0.0 { p * pow(k) - emi_f * (pow(k) - 1.0) / r } else { p - emi_f * k };
    let end_idx = (sm as i32 - 1) + tenure as i32 - 1;
    let end = format!("{:04}-{:02}", sy + end_idx.div_euclid(12), end_idx.rem_euclid(12) + 1);
    Some(LoanCalc {
        emi: emi_f.round() as i64,
        paid,
        left: tenure - paid,
        tenure,
        balance: bal.max(0.0).round() as i64,
        end,
        interest: (emi_f * n - p).round() as i64,
    })
}

pub fn parse_ym(s: &str) -> Option<(i32, u32)> {
    let (y, m) = s.split_once('-')?;
    let (y, m) = (y.parse().ok()?, m.get(..2).unwrap_or(m).parse::<u32>().ok()?);
    (1..=12).contains(&m).then_some((y, m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_design_example() {
        // home loan: 25,00,000 at 8.5% over 180 months, started 2019-04, viewed in oct 2026
        let c = compute(2_500_000_00, 8.5, 180, "2019-04", None, (2026, 10)).unwrap();
        assert_eq!(c.paid, 91);
        assert_eq!(c.left, 89);
        assert_eq!(c.end, "2034-03");
        assert!((c.emi - 2_461_800).abs() < 20_000, "emi {}", c.emi);
    }

    #[test]
    fn zero_rate_and_not_started() {
        let c = compute(120_000_00, 0.0, 12, "2027-01", None, (2026, 10)).unwrap();
        assert_eq!((c.paid, c.balance, c.emi), (0, 120_000_00, 10_000_00));
    }
}
