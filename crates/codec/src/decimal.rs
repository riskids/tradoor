//! Exchange decimal strings as integer mantissas.

/// `"64123.40"` → `(641234, 1)`. Trailing fractional zeros are dropped, so the
/// scale is the precision the number actually has. No exponent notation: no
/// exchange sends it, and accepting it would hide a malformed field.
pub fn parse_decimal(s: &str) -> Option<(i64, u8)> {
    let s = s.trim();
    let (negative, body) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if (int.is_empty() && frac.is_empty()) || !int.bytes().chain(frac.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > 18 {
        return None;
    }
    let mut mantissa: i64 = 0;
    for c in int.bytes().chain(frac.bytes()) {
        mantissa = mantissa.checked_mul(10)?.checked_add(i64::from(c - b'0'))?;
    }
    Some((if negative { -mantissa } else { mantissa }, frac.len() as u8))
}

pub fn pow10(scale: u8) -> Option<i64> {
    10i64.checked_pow(u32::from(scale))
}

/// Re-express a mantissa at a finer scale. Refuses to coarsen: that would drop
/// digits the exchange sent.
pub fn rescale(mantissa: i64, from: u8, to: u8) -> Option<i64> {
    if to < from {
        return None;
    }
    mantissa.checked_mul(pow10(to - from)?)
}

pub fn to_f64(mantissa: i64, scale: u8) -> f64 {
    mantissa as f64 / 10f64.powi(i32::from(scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exchange_strings() {
        assert_eq!(parse_decimal("64123.40"), Some((641234, 1)));
        assert_eq!(parse_decimal("0.00001234"), Some((1234, 8)));
        assert_eq!(parse_decimal("12.00000000"), Some((12, 0)));
        assert_eq!(parse_decimal("-1.5"), Some((-15, 1)));
        assert_eq!(parse_decimal("7"), Some((7, 0)));
        assert_eq!(parse_decimal(".5"), Some((5, 1)));
        assert_eq!(parse_decimal("1e5"), None);
        assert_eq!(parse_decimal(""), None);
        assert_eq!(parse_decimal("99999999999999999999"), None);
    }

    #[test]
    fn rescales_only_finer() {
        assert_eq!(rescale(641234, 1, 2), Some(6412340));
        assert_eq!(rescale(641234, 2, 1), None);
        assert_eq!(to_f64(6412340, 2), 64123.4);
        assert_eq!(to_f64(1234, 8), 0.00001234);
    }
}
