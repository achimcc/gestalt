//! What kind of string something is — without saying what it says.
//!
//! The class is printed in place of the value, and it decides whether an
//! object key is part of the schema (`name`, `apiKey`) or part of the data
//! (a user id, a token, an e-mail address used as a map key). Data keys are
//! never printed; they are folded into one `{*}` path.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Empty,
    Digits,
    Uuid,
    Hex,
    Ip,
    Date,
    DateTime,
    Url,
    Email,
    Path,
    Token,
    Text,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Empty => "empty",
            Class::Digits => "digits",
            Class::Uuid => "uuid",
            Class::Hex => "hex",
            Class::Ip => "ip",
            Class::Date => "date",
            Class::DateTime => "datetime",
            Class::Url => "url",
            Class::Email => "email",
            Class::Path => "path",
            Class::Token => "token",
            Class::Text => "text",
        }
    }

    /// Whether a key of this class names a *record* rather than a *field*.
    ///
    /// Leaning towards "data" is the safe side: a field name folded into
    /// `{*}` costs readability, a token printed as a field name costs a
    /// rotation.
    /// Values of these classes are credentials often enough that `--show`
    /// refuses them (B91): a key, a hash, a URL with a passkey, an id.
    pub fn is_secret_like(self) -> bool {
        matches!(self, Class::Token | Class::Hex | Class::Url | Class::Uuid)
    }

    pub fn is_data_like(self) -> bool {
        !matches!(self, Class::Empty | Class::Text)
    }
}

pub fn classify(s: &str) -> Class {
    let b = s.as_bytes();
    if b.is_empty() {
        return Class::Empty;
    }
    if b.iter().all(u8::is_ascii_digit) {
        return Class::Digits;
    }
    if is_uuid(b) {
        return Class::Uuid;
    }
    if let Some(c) = date_class(b) {
        return c;
    }
    if b.len() >= 8 && b.iter().all(u8::is_ascii_hexdigit) && b.iter().any(u8::is_ascii_digit) {
        return Class::Hex;
    }
    if is_ip(s) {
        return Class::Ip;
    }
    if is_url(s) {
        return Class::Url;
    }
    if is_email(s) {
        return Class::Email;
    }
    if b.len() > 1 && b[0] == b'/' {
        return Class::Path;
    }
    if looks_like_token(b) {
        return Class::Token;
    }
    Class::Text
}

fn is_uuid(b: &[u8]) -> bool {
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn date_class(b: &[u8]) -> Option<Class> {
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let is_date = (0..4).all(d)
        && b.get(4) == Some(&b'-')
        && d(5)
        && d(6)
        && b.get(7) == Some(&b'-')
        && d(8)
        && d(9);
    if !is_date {
        return None;
    }
    if b.len() == 10 {
        return Some(Class::Date);
    }
    let is_time =
        matches!(b.get(10), Some(b'T' | b' ')) && d(11) && d(12) && b.get(13) == Some(&b':');
    is_time.then_some(Class::DateTime)
}

fn is_ip(s: &str) -> bool {
    // With or without a port or a prefix length: `10.0.0.5:8080`, `fd00::1/64`.
    let host = s.split('/').next().unwrap_or(s);
    host.parse::<std::net::IpAddr>().is_ok() || host.parse::<std::net::SocketAddr>().is_ok()
}

fn is_url(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !rest.is_empty()
}

fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !s.chars().any(char::is_whitespace)
}

/// Whether an object key is printed as a field name. Everything else is
/// folded into `{*}` (B134, homeserver audit 3: "when in doubt, fold").
///
/// Up to 0.2.0 a key was printed unless it *looked like* data — and a
/// 15-character key, a letters-only key, `Bearer …`, `user:pass` or
/// `name@localhost` did not. Now the burden is reversed: a key is printed
/// only if it *looks like* a field name, and a field name that folds costs
/// readability, a secret that does not costs a rotation.
///
/// A field name is an identifier (`[A-Za-z_][A-Za-z0-9_-]{0,39}`) that no
/// class claims (hex, token, …), with at most two runs of digits (`ipv4`,
/// `x509Certificate`, `address_line_2` — not `a1b2c3d`) and at most two
/// short camel-case humps (`getElementsByTagName` — not `QxmKpRsTvWza…`).
/// What remains undetectable is a short word-like secret such as
/// `hunter2`; that is the classifier's limit, not a field name.
pub fn is_field_name(key: &str) -> bool {
    let b = key.as_bytes();
    let shape_ok = (1..=40).contains(&b.len())
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-');
    shape_ok && !classify(key).is_data_like() && digit_runs(b) <= 2 && short_humps(b) <= 2
}

fn digit_runs(b: &[u8]) -> usize {
    b.iter()
        .enumerate()
        .filter(|&(i, c)| c.is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()))
        .count()
}

/// Camel-case words of one or two letters (`Kp`, `Rs`, `Q`): a random
/// mixed-case string is full of them, a field name has one or two (`By`, `Id`).
fn short_humps(b: &[u8]) -> usize {
    // Word starts: an upper-case letter after a lower-case letter or a digit.
    let starts: Vec<usize> = (1..b.len())
        .filter(|&i| {
            b[i].is_ascii_uppercase()
                && (b[i - 1].is_ascii_lowercase() || b[i - 1].is_ascii_digit())
        })
        .collect();
    starts
        .iter()
        .filter(|&&i| {
            let lower = b[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_lowercase())
                .count();
            lower <= 1
        })
        .count()
}

/// Long, no spaces, letters AND digits: an API key, a passkey, a session id,
/// a JWT. Plain identifiers like `ENABLE_ADDITIONAL_METRICS` have no digit
/// and stay text.
fn looks_like_token(b: &[u8]) -> bool {
    b.len() >= 16
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_-+/=.~".contains(c))
        && b.iter().any(u8::is_ascii_digit)
        && b.iter().any(u8::is_ascii_alphabetic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        let cases = [
            ("", Class::Empty),
            ("8080", Class::Digits),
            ("123e4567-e89b-12d3-a456-426614174000", Class::Uuid),
            ("deadbeef01", Class::Hex),
            ("2026-09-18", Class::Date),
            ("2026-09-18T12:00:00Z", Class::DateTime),
            ("2026-09-18 12:00:00", Class::DateTime),
            ("https://example.org/announce?passkey=abc", Class::Url),
            ("user@example.org", Class::Email),
            ("10.0.0.5", Class::Ip),
            ("10.0.0.5:8080", Class::Ip),
            ("fd00::1/64", Class::Ip),
            ("/var/lib/radarr", Class::Path),
            ("aB3dE5gH7jK9mN1pQ3rS", Class::Token),
            ("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc123", Class::Token),
            ("ENABLE_ADDITIONAL_METRICS", Class::Text),
            ("apiKey", Class::Text),
            ("Hello world", Class::Text),
            ("1.2.3", Class::Text),
        ];
        for (s, want) in cases {
            assert_eq!(classify(s), want, "{s:?}");
        }
    }

    #[test]
    fn field_names_are_not_data() {
        for k in [
            "name",
            "apiKey",
            "api_key",
            "x-forwarded-for",
            "Version",
            "v2",
        ] {
            assert!(!classify(k).is_data_like(), "{k}");
            assert!(is_field_name(k), "{k}");
        }
    }

    #[test]
    fn doubtful_keys_are_not_field_names() {
        for k in [
            "Qx7mK2pR9sT4vW8",
            "QxmKpRsTvWzaBcDeFgHj",
            "a1b2c3d",
            "admin:pw",
            "Bearer x",
            "a%2Bb",
            "a@localhost",
            "aa:bb:cc",
            "x y",
            "a.b",
            "",
            "1abc",
            "a\u{9b}b",
            "b\u{202e}evil",
        ] {
            assert!(!is_field_name(k), "{k:?}");
        }
    }
}
