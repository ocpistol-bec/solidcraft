//! Equation constants printed as the reference reader prints them: Python's
//! `repr(round(value, 12))`, so equations compare as text across implementations.

/// Python's `repr(round(x, 12))`.
pub(super) fn rounded_repr(x: f64) -> String {
    python_repr(round12(x))
}

/// `round(x, 12)`: the double nearest the correctly rounded 12-decimal value.
fn round12(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.12}").parse().unwrap_or(x)
}

/// Python's float `repr`: the shortest round-trip digits, positional for decimal exponents in
/// -4..16 (with at least one fractional digit), scientific with a signed two-digit exponent
/// otherwise.
fn python_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let (digits, exp) = shortest_digits(x.abs());
    if (-4..16).contains(&exp) {
        format!("{sign}{}", positional(&digits, exp))
    } else {
        format!("{sign}{}", scientific(&digits, exp))
    }
}

/// The shortest round-trip decimal digits of `x ≥ 0` and the exponent of the first digit.
fn shortest_digits(x: f64) -> (String, i32) {
    let text = format!("{x:e}");
    let (mantissa, exp) = text.split_once('e').unwrap_or((text.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exp.parse().unwrap_or(0))
}

/// `digits × 10^(exp - len + 1)` written out, e.g. ("15", 1) → "15.0", ("15", -2) → "0.015".
fn positional(digits: &str, exp: i32) -> String {
    if exp < 0 {
        let zeros = "0".repeat((-exp - 1) as usize);
        return format!("0.{zeros}{digits}");
    }
    let int_len = exp as usize + 1;
    if digits.len() <= int_len {
        return format!("{digits}{}.0", "0".repeat(int_len - digits.len()));
    }
    format!("{}.{}", &digits[..int_len], &digits[int_len..])
}

/// Scientific form, e.g. ("15", 20) → "1.5e+20", ("1", -5) → "1e-05".
fn scientific(digits: &str, exp: i32) -> String {
    let (lead, rest) = digits.split_at(1.min(digits.len()));
    let fraction = if rest.is_empty() {
        String::new()
    } else {
        format!(".{rest}")
    };
    let exp_sign = if exp < 0 { '-' } else { '+' };
    format!("{lead}{fraction}e{exp_sign}{:02}", exp.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_repr() {
        let cases = [
            (1.0, "1.0"),
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (0.5, "0.5"),
            (2.54, "2.54"),
            (-12.5, "-12.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (123456.789, "123456.789"),
        ];
        for (x, want) in cases {
            assert_eq!(python_repr(x), want, "{x}");
        }
    }

    #[test]
    fn rounds_to_twelve_decimals() {
        assert_eq!(rounded_repr(0.1 + 0.2), "0.3");
        assert_eq!(rounded_repr(1e-13), "0.0");
        assert_eq!(rounded_repr(0.39370078740157477), "0.393700787402");
    }
}
