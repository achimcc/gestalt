//! gestalt — the shape of a JSON document, without its values.
//!
//! Strings and numbers are replaced by their type, length and a coarse class
//! (`hex`, `token`, `url`, …). Booleans and nulls are shown, because a bit is
//! not a secret and `"allowPasswordLogin": false` is often the point.
//! Object keys that look like data rather than field names are folded into
//! `{*}`. A value is printed only for a path named with `--show`.

pub mod classify;
pub mod formate;
pub mod render;
pub mod shape;

use std::collections::HashSet;

pub struct Options {
    pub show: HashSet<String>,
    /// Read the input as this format instead of detecting it.
    pub format: Option<formate::Format>,
    /// Paths whose values are printed as SHA-256 only — the answer to "is it
    /// the same secret?" without the secret.
    pub hash: HashSet<String>,
    pub numbers: bool,
}

pub struct Report {
    pub text: String,
    /// `--show` paths that did not occur. Reported, because a filter that
    /// matches nothing looks exactly like data that has nothing.
    pub unmatched: Vec<String>,
    /// `--show` paths that only ever held objects or arrays.
    pub containers: Vec<String>,
    /// `--show` paths refused because a value there looks like a secret
    /// (class token/hex/url/uuid, or a key name like `apiKey`).
    pub refused: Vec<String>,
}

#[derive(Debug)]
pub enum Error {
    Empty,
    NotJson {
        documents: usize,
        reason: String,
        bytes: usize,
        looks_like: &'static str,
    },
    /// No reader took the input. `tried` names the formats, in order.
    Unreadable {
        tried: Vec<&'static str>,
        bytes: usize,
        looks_like: &'static str,
    },
    /// `--format` named a format the input is not.
    NotFormat {
        format: &'static str,
        reason: &'static str,
        bytes: usize,
        looks_like: &'static str,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Empty => write!(f, "input is empty"),
            Error::NotJson {
                documents,
                reason,
                bytes,
                looks_like,
            } => {
                write!(
                    f,
                    "input is not JSON ({reason}); {bytes} bytes, looks like {looks_like}"
                )?;
                if *documents > 0 {
                    write!(f, ", after {documents} valid document(s)")?;
                }
                Ok(())
            }
            Error::Unreadable {
                tried,
                bytes,
                looks_like,
            } => {
                // "not JSON" stays the first word: it is what a reader of
                // the 0.3 messages (and the tests) look for.
                let rest: Vec<&str> = tried.iter().copied().filter(|t| *t != "json").collect();
                if rest.is_empty() {
                    write!(
                        f,
                        "input is not JSON; {bytes} bytes, looks like {looks_like}"
                    )
                } else {
                    write!(
                        f,
                        "input is not JSON (nor {}); {bytes} bytes, looks like {looks_like}",
                        rest.join(", ")
                    )
                }
            }
            Error::NotFormat {
                format,
                reason,
                bytes,
                looks_like,
            } => write!(
                f,
                "input is not {format} ({reason}); {bytes} bytes, looks like {looks_like}"
            ),
        }
    }
}

/// serde_json's messages name a position, never the input — so they are
/// safe to print. The input itself is only ever characterised.
fn looks_like(input: &[u8]) -> &'static str {
    let start = input.trim_ascii_start();
    let Ok(text) = std::str::from_utf8(input) else {
        return "binary";
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let sections = lines
        .iter()
        .filter(|l| l.starts_with('[') && l.ends_with(']'))
        .count();
    let pairs = lines.iter().filter(|l| l.contains('=')).count();
    let yaml_ish = lines.iter().any(|l| l.contains(": ") || l.ends_with(':'))
        && text
            .lines()
            .any(|l| l.starts_with(' ') && !l.trim().is_empty());
    match start.first() {
        None => "whitespace",
        Some(b'<') => "HTML or XML",
        Some(b'{') => "truncated or broken JSON",
        Some(b'[') if sections > 0 => {
            "an INI or unit file (sections, but not every line is key=value)"
        }
        Some(b'[') => "truncated or broken JSON",
        _ if !lines.is_empty() && pairs * 2 >= lines.len() => "key=value lines (not all of them)",
        _ if yaml_ish => "YAML-like text",
        _ => "text",
    }
}

/// Reads one JSON document or several in a row (JSON lines, concatenated
/// output of a loop) and describes their common shape.
pub fn describe(input: &[u8], opts: &Options) -> Result<Report, Error> {
    let bytes = input.len();
    let parsed = match formate::parse(input, opts.format) {
        Ok(p) => p,
        Err(formate::ParseError::Empty) => return Err(Error::Empty),
        Err(formate::ParseError::NotUtf8) => {
            return Err(Error::Unreadable {
                tried: vec![],
                bytes,
                looks_like: "binary",
            });
        }
        Err(formate::ParseError::NotJson { documents, reason }) => {
            return Err(Error::NotJson {
                documents,
                reason,
                bytes,
                looks_like: looks_like(input),
            });
        }
        Err(formate::ParseError::Unreadable { tried }) => {
            return Err(Error::Unreadable {
                tried,
                bytes,
                looks_like: looks_like(input),
            });
        }
        Err(formate::ParseError::NotForced { format, reason }) => {
            return Err(Error::NotFormat {
                format: format.name(),
                reason,
                bytes,
                looks_like: looks_like(input),
            });
        }
    };
    let mut collector = shape::Collector::new(&opts.show, opts.numbers);
    collector.hash = &opts.hash;
    for v in &parsed.documents {
        collector.add_document(v);
    }
    let mut unmatched: Vec<String> = opts
        .show
        .iter()
        .chain(opts.hash.iter())
        .filter(|p| !collector.paths.contains_key(*p))
        .cloned()
        .collect();
    unmatched.sort();
    let mut containers: Vec<String> = opts
        .show
        .iter()
        .filter(|p| {
            collector
                .paths
                .get(*p)
                .is_some_and(shape::Stats::only_containers)
        })
        .cloned()
        .collect();
    containers.sort();
    // A refused path shows nothing — not even the harmless values seen
    // before the one that looked like a secret.
    for st in collector.paths.values_mut() {
        if st.refused {
            st.values.clear();
            st.values_more = 0;
        }
    }
    let mut refused: Vec<String> = collector
        .paths
        .iter()
        .filter(|(_, st)| st.refused)
        .map(|(p, _)| p.clone())
        .collect();
    refused.sort();
    let header = render::Header {
        documents: collector.documents,
        bytes,
        format: (parsed.format != formate::Format::Json).then_some(parsed.format.name()),
        comment_lines: parsed.comment_lines,
    };
    let text = render::render(&collector.paths, &header);
    Ok(Report {
        text,
        unmatched,
        containers,
        refused,
    })
}
