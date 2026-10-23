//! The formats gestalt reads besides JSON. Each one is turned into a
//! `serde_json::Value` and then described by the same collector, so the
//! rules about what is shown — nothing, unless asked by path — hold for all.
//!
//! Detection is by content, in a fixed order, with three guards so that
//! broken JSON cannot fall through to a more forgiving reader:
//!
//! - input starting with `{` is JSON or nothing (`{"k": 1, oops}` is a valid
//!   YAML flow mapping);
//! - input starting with `[` may be JSON, a unit file, TOML or INI — never
//!   YAML;
//! - once JSON has read one document and then fails, it stays a JSON error.
//!
//! The unit reader runs before TOML because `[Timer]\nOnBootSec=30` is
//! valid TOML too.

pub mod ini;
pub mod toml;
pub mod yaml;

use serde_json::Value;

pub use ini::{Dialect, Reject};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Toml,
    Ini,
    Env,
    Unit,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Yaml => "yaml",
            Format::Toml => "toml",
            Format::Ini => "ini",
            Format::Env => "env",
            Format::Unit => "unit",
        }
    }

    pub fn parse(s: &str) -> Option<Format> {
        Some(match s {
            "json" => Format::Json,
            "yaml" | "yml" => Format::Yaml,
            "toml" => Format::Toml,
            "ini" => Format::Ini,
            "env" | "dotenv" => Format::Env,
            "unit" | "systemd" => Format::Unit,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct Parsed {
    pub format: Format,
    pub documents: Vec<Value>,
    /// Lines starting with `#` or `;` (after whitespace). Never shown: a
    /// comment can carry a password as easily as a value can.
    pub comment_lines: usize,
}

/// What went wrong, without the input.
#[derive(Debug)]
pub enum ParseError {
    Empty,
    /// JSON was the only candidate (or the forced format) and failed.
    NotJson {
        documents: usize,
        reason: String,
    },
    /// Every candidate declined.
    Unreadable {
        tried: Vec<&'static str>,
    },
    /// `--format X` and the input is not X.
    NotForced {
        format: Format,
        reason: &'static str,
    },
    NotUtf8,
}

fn comment_lines(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with('#') || t.starts_with(';')
        })
        .count()
}

fn read_json(input: &[u8]) -> Result<Vec<Value>, (usize, String)> {
    let mut docs = Vec::new();
    for doc in serde_json::Deserializer::from_slice(input).into_iter::<Value>() {
        match doc {
            Ok(v) => docs.push(v),
            // serde_json's messages name a position, never the input.
            Err(e) => return Err((docs.len(), e.to_string())),
        }
    }
    Ok(docs)
}

fn read_one(
    format: Format,
    input: &[u8],
    text: &str,
    forced: bool,
) -> Result<Vec<Value>, &'static str> {
    let dialect = |f: Format| match f {
        Format::Unit => Dialect::Unit,
        Format::Ini => Dialect::Ini,
        _ => Dialect::Env,
    };
    match format {
        Format::Json => read_json(input).map_err(|_| "not json"),
        Format::Toml => toml::read(text).map(|v| vec![v]).map_err(|r| r.0),
        Format::Yaml => yaml::read(text).map_err(|r| r.0),
        Format::Unit | Format::Ini | Format::Env => {
            // Forced: the dialect is given, only a structural contradiction
            // rejects. Detected: the reader decides, and must agree.
            let want = forced.then(|| dialect(format));
            let (got, v) = ini::read(text, want).map_err(|r| r.0)?;
            if got == dialect(format) {
                Ok(vec![v])
            } else {
                Err("other dialect")
            }
        }
    }
}

/// The first byte that decides between JSON and the rest: after a BOM,
/// blank lines and comment lines (`#`, `;`). Without this, `\u{feff}{…}`
/// or `# note\n{…}` slipped past the `{` guard into YAML (review 2026-10-23).
fn first_significant_byte(text: &str) -> Option<u8> {
    text.lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .and_then(|l| l.as_bytes().first().copied())
}

/// Reads the input in the forced format, or in the first format that takes
/// the whole of it.
pub fn parse(input: &[u8], forced: Option<Format>) -> Result<Parsed, ParseError> {
    if input.trim_ascii().is_empty() {
        return Err(ParseError::Empty);
    }
    let Ok(text) = std::str::from_utf8(input) else {
        return Err(ParseError::NotUtf8);
    };
    // A byte-order mark is not whitespace to the parsers; drop it once.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let input = text.as_bytes();
    let comments = comment_lines(text);
    let done = |format: Format, documents: Vec<Value>| Parsed {
        format,
        documents,
        comment_lines: if format == Format::Json { 0 } else { comments },
    };

    if let Some(f) = forced {
        return match (f, read_one(f, input, text, true)) {
            (_, Ok(docs)) => Ok(done(f, docs)),
            (Format::Json, Err(_)) => {
                let (documents, reason) = read_json(input).err().unwrap_or_default();
                Err(ParseError::NotJson { documents, reason })
            }
            (_, Err(reason)) => Err(ParseError::NotForced { format: f, reason }),
        };
    }

    let first = first_significant_byte(text);
    match read_json(input) {
        Ok(docs) => return Ok(done(Format::Json, docs)),
        Err((documents, reason)) if documents > 0 || first == Some(b'{') => {
            return Err(ParseError::NotJson { documents, reason });
        }
        Err(_) => {}
    }
    let candidates: &[Format] = if first == Some(b'[') {
        &[Format::Unit, Format::Toml, Format::Ini]
    } else {
        &[
            Format::Unit,
            Format::Toml,
            Format::Ini,
            Format::Env,
            Format::Yaml,
        ]
    };
    let mut tried = vec!["json"];
    for &f in candidates {
        tried.push(f.name());
        if let Ok(docs) = read_one(f, input, text, false) {
            return Ok(done(f, docs));
        }
    }
    Err(ParseError::Unreadable { tried })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brace_means_json_or_nothing() {
        assert!(matches!(
            parse(b"{\"k\": 1, oops}", None),
            Err(ParseError::NotJson { documents: 0, .. })
        ));
    }

    #[test]
    fn bracket_never_reaches_yaml() {
        let e = parse(b"[1, 2,", None).unwrap_err();
        match e {
            ParseError::Unreadable { tried } => assert!(!tried.contains(&"yaml"), "{tried:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unit_before_toml() {
        let p = parse(b"[Timer]\nOnBootSec=30\nPersistent=true\n", None).unwrap();
        assert_eq!(p.format, Format::Unit);
    }

    #[test]
    fn comments_counted_for_every_format_but_json() {
        assert_eq!(parse(b"# c\na: 1\n", None).unwrap().comment_lines, 1);
        assert_eq!(parse(b"{\"a\": 1}", None).unwrap().comment_lines, 0);
    }
}
