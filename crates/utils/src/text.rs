//! Small text helpers: hex, percent-encoding, html escaping, email check.

pub fn to_hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn hexv(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|v| v as u8)
}

/// Decode `%XX` escapes (and `+` as space when `plus_is_space`). Invalid escapes pass through.
pub fn percent_decode(s: &str, plus_is_space: bool) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() && hexv(b[i + 1]).is_some() && hexv(b[i + 2]).is_some() => {
                out.push(hexv(b[i + 1]).unwrap() * 16 + hexv(b[i + 2]).unwrap());
                i += 3;
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode everything except unreserved characters (and `/` if `keep_slash`).
pub fn percent_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~') || (keep_slash && c == b'/') {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{:02X}", c));
        }
    }
    out
}

pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `local@domain.tld` shape check (no regex dependency).
pub fn is_email_address(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else { return false };
    if local.is_empty() || domain.contains('@') {
        return false;
    }
    if !local.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c)) {
        return false;
    }
    let Some((host, tld)) = domain.rsplit_once('.') else { return false };
    !host.is_empty()
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Parse `YYYY-MM-DD` into (year, month, day), validating ranges.
pub fn parse_date(s: &str) -> Option<(i64, u32, u32)> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
        return None;
    }
    Some((y, m, d))
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Civil date for days since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Unix seconds at 00:00:00 UTC of the given date.
pub fn date_to_unix(y: i64, m: u32, d: u32) -> u64 {
    (days_from_civil(y, m, d) * 86400).max(0) as u64
}

/// `YYYY-MM-DD` (UTC) for a unix timestamp.
pub fn unix_to_date(ts: u64) -> String {
    let (y, m, d) = civil_from_days((ts / 86400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// iCalendar UTC timestamp (`YYYYMMDDTHHMMSSZ`).
pub fn unix_to_ical(ts: u64) -> String {
    let (y, m, d) = civil_from_days((ts / 86400) as i64);
    let s = ts % 86400;
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex() {
        assert_eq!(to_hex(&[72, 69, 76, 76, 79]), "48454c4c4f");
        assert_eq!(from_hex("48454c4c4f").unwrap(), vec![72, 69, 76, 76, 79]);
        assert!(from_hex("zz").is_none());
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("a%20b+c%2", true), "a b c%2");
        assert_eq!(percent_decode("a%20b+c", false), "a b+c");
        assert_eq!(percent_decode("%e2%82%ac", false), "€");
        assert_eq!(percent_encode("a b/é", true), "a%20b/%C3%A9");
        assert_eq!(percent_decode("%", false), "%");
        assert_eq!(percent_decode("%4", false), "%4");
        assert_eq!(percent_decode("%41", false), "A");
    }

    #[test]
    fn email() {
        assert!(is_email_address("john@example.com"));
        assert!(!is_email_address("john@example"));
        assert!(!is_email_address("@example.com"));
        assert!(!is_email_address("a@b@example.com"));
    }

    #[test]
    fn dates() {
        assert_eq!(parse_date("2024-02-29"), Some((2024, 2, 29)));
        assert!(parse_date("2023-02-29").is_none());
        let ts = date_to_unix(2026, 11, 16);
        assert_eq!(unix_to_date(ts), "2026-11-16");
        assert_eq!(unix_to_ical(ts + 3661), "20261116T010101Z");
        assert_eq!(date_to_unix(1970, 1, 1), 0);
    }

    #[test]
    fn escape() {
        assert_eq!(html_escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }
}
