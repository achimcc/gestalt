//! gestalt — the shape of a JSON document, without its values.
//!
//! Strings and numbers are replaced by their type, length and a coarse class
//! (`hex`, `token`, `url`, …). Booleans and nulls are shown, because a bit is
//! not a secret and `"allowPasswordLogin": false` is often the point.
//! Object keys that look like data rather than field names are folded into
//! `{*}`. A value is printed only for a path named with `--show`.

pub mod classify;
pub mod render;
pub mod shape;

use std::collections::HashSet;

use serde_json::Value;

pub struct Options {
    pub show: HashSet<String>,
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
        }
    }
}

/// serde_json's messages name a position, never the input — so they are
/// safe to print. The input itself is only ever characterised.
fn looks_like(input: &[u8]) -> &'static str {
    let start = input.trim_ascii_start();
    match start.first() {
        None => "whitespace",
        Some(b'<') => "HTML or XML",
        Some(b'{' | b'[') => "truncated or broken JSON",
        _ if std::str::from_utf8(input).is_err() => "binary",
        _ => "text",
    }
}

/// Reads one JSON document or several in a row (JSON lines, concatenated
/// output of a loop) and describes their common shape.
pub fn describe(input: &[u8], opts: &Options) -> Result<Report, Error> {
    if input.trim_ascii().is_empty() {
        return Err(Error::Empty);
    }
    let mut collector = shape::Collector::new(&opts.show, opts.numbers);
    collector.hash = &opts.hash;
    for doc in serde_json::Deserializer::from_slice(input).into_iter::<Value>() {
        match doc {
            Ok(v) => collector.add_document(&v),
            Err(e) => {
                return Err(Error::NotJson {
                    documents: collector.documents,
                    reason: e.to_string(),
                    bytes: input.len(),
                    looks_like: looks_like(input),
                });
            }
        }
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
    let text = render::render(&collector.paths, collector.documents, input.len());
    Ok(Report {
        text,
        unmatched,
        containers,
        refused,
    })
}
