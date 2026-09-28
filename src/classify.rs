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
        }
    }
}
