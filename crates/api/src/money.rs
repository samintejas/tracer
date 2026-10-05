use serde::{Deserialize, Deserializer, Serializer};

/// Parse `"12.50"`, `"-0.07"`, `"1,23,456"` into minor units. At most two decimals.
pub fn parse_minor(s: &str) -> Result<i64, String> {
    let cleaned: String = s.trim().chars().filter(|c| *c != ',' && *c != '_' && !c.is_whitespace()).collect();
    let s = cleaned.trim_start_matches(['₹', '$', '€']);
    let (neg, s) = match s.strip_prefix('-').or_else(|| s.strip_prefix('−')) {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if (whole.is_empty() && frac.is_empty())
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
    {
        return Err(format!("invalid amount '{s}'"));
    }
    if frac.len() > 2 {
        return Err(format!("amount '{s}' has more than 2 decimal places"));
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| "amount too large".to_string())? };
    let frac: i64 = format!("{frac:0<2}").parse().unwrap();
    let v = whole.checked_mul(100).and_then(|v| v.checked_add(frac)).ok_or("amount too large")?;
    Ok(if neg { -v } else { v })
}

/// Minor units to `"12.50"`.
pub fn format_minor(c: i64) -> String {
    let sign = if c < 0 { "-" } else { "" };
    let a = c.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

/// Group digits for display: Indian (`1,23,456`) or western (`123,456`). Whole units, rounded.
pub fn group_digits(minor: i64, indian: bool) -> String {
    let n = ((minor.unsigned_abs() + 50) / 100).to_string();
    let b = n.as_bytes();
    if b.len() <= 3 {
        return n;
    }
    let (head, tail) = n.split_at(n.len() - 3);
    let mut out = String::new();
    let step = if indian { 2 } else { 3 };
    let h = head.as_bytes();
    for (i, ch) in h.iter().enumerate() {
        if i > 0 && (h.len() - i) % step == 0 {
            out.push(',');
        }
        out.push(*ch as char);
    }
    format!("{out},{tail}")
}

/// An amount in minor units that deserialises from a JSON number or string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Money(pub i64);

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let s = match v {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s,
            _ => return Err(serde::de::Error::custom("amount must be a number or string")),
        };
        parse_minor(&s).map(Money).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for Money {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_minor(self.0))
    }
}

/// `serde(with = "money::opt")` for `Option<Money>`-like fields stored as `Option<i64>`.
pub mod opt {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(c) => s.serialize_some(&format_minor(*c)),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        Ok(Option::<Money>::deserialize(d)?.map(|m| m.0))
    }
}

/// `serde(with = "money::val")` for a plain `i64` of minor units.
pub mod val {
    use super::*;
    pub fn serialize<S: Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_minor(*v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        Ok(Money::deserialize(d)?.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        assert_eq!(parse_minor("12.5").unwrap(), 1250);
        assert_eq!(parse_minor("-0.07").unwrap(), -7);
        assert_eq!(parse_minor("₹1,23,456").unwrap(), 12_345_600);
        assert!(parse_minor("1.234").is_err());
        assert!(parse_minor("abc").is_err());
        assert_eq!(format_minor(-7), "-0.07");
    }

    #[test]
    fn groups() {
        assert_eq!(group_digits(365_580_00, true), "3,65,580");
        assert_eq!(group_digits(365_580_00, false), "365,580");
        assert_eq!(group_digits(99_00, true), "99");
        assert_eq!(group_digits(25_00_000_00, true), "25,00,000");
    }
}
