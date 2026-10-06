//! How numbers and dates are written. The stored amounts never change; only the display does.

use tracer_api::money::group_digits;

pub fn symbol(cur: &str) -> &'static str {
    match cur {
        "usd" => "$",
        "eur" => "€",
        _ => "₹",
    }
}

/// `₹3,65,580` (indian grouping for inr, western otherwise). Whole units, rounded.
pub fn money(minor: i64, cur: &str) -> String {
    format!("{}{}", symbol(cur), group_digits(minor, cur == "inr"))
}

/// Same with the sign written as a true minus: `−₹3,240`.
pub fn signed(minor: i64, cur: &str) -> String {
    if minor < 0 { format!("−{}", money(minor, cur)) } else { money(minor, cur) }
}

/// `2026-10-04` to `04 oct`.
pub fn day(date: &str) -> String {
    const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let (m, d) = (date.get(5..7).and_then(|m| m.parse::<usize>().ok()), date.get(8..10));
    match (m, d) {
        (Some(m), Some(d)) if (1..=12).contains(&m) => format!("{d} {}", M[m - 1]),
        _ => date.to_string(),
    }
}

/// `2026-10` to `oct`.
pub fn month(ym: &str) -> String {
    day(&format!("{ym}-01")).split(' ').nth(1).unwrap_or(ym).to_string()
}

/// A plain number in major units for an input, without grouping: `3240` or `3240.5`.
pub fn plain(minor: i64) -> String {
    let s = tracer_api::money::format_minor(minor.abs());
    s.strip_suffix(".00").map(String::from).unwrap_or(s)
}

pub fn today() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// `YYYY-MM-DD` that many days ago.
pub fn days_ago(n: i32) -> String {
    let d = js_sys::Date::new_0();
    d.set_date((d.get_date() as i32 - n) as u32);
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// `2034-03` to `mar 2034`.
pub fn month_year(ym: &str) -> String {
    format!("{} {}", month(ym), ym.get(..4).unwrap_or(""))
}

/// `5` to `5th`.
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// `45%` of a whole, `0%` when there is none.
pub fn pct(part: i64, whole: i64) -> String {
    if whole == 0 { "0%".into() } else { format!("{}%", ((part as f64) / (whole as f64) * 100.0).round() as i64) }
}

/// `5 oct`, with the year when it is not this year: `5 jan 2027`.
pub fn day_label(date: &str) -> String {
    const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let parts = (date.get(..4), date.get(5..7).and_then(|m| m.parse::<usize>().ok()), date.get(8..10).and_then(|d| d.parse::<u32>().ok()));
    match parts {
        (Some(y), Some(m), Some(d)) if (1..=12).contains(&m) => {
            if today().starts_with(y) { format!("{d} {}", M[m - 1]) } else { format!("{d} {} {y}", M[m - 1]) }
        }
        _ => date.to_string(),
    }
}

/// `YYYY-MM-DD` today plus that many months, on the last day of the month when it has no such day.
pub fn in_months(n: u32) -> String {
    let d = js_sys::Date::new_0();
    let day = d.get_date();
    d.set_date(1);
    d.set_month(d.get_month() + n);
    let last = js_sys::Date::new_with_year_month_day(d.get_full_year(), d.get_month() as i32 + 1, 0).get_date();
    d.set_date(day.min(last));
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}
