//! INI, systemd unit files and `.env` — one small reader, three dialects.
//!
//! The dialects are told apart by what the document contains: a section
//! header makes it INI or unit, no section makes it `.env`. No crate reads
//! all three the way they are written in practice (continuation lines,
//! repeated keys, `export`), so this is our own.

use serde_json::{Map, Value};

/// Which dialect the document turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    /// Every section is a systemd one; every key is a list, because systemd
    /// lets every setting repeat and the path must not depend on how often.
    Unit,
    /// Sections with scalar keys; a repeated key overwrites.
    Ini,
    /// No sections, `KEY=value` lines, optional `export`.
    Env,
}

/// Why a document is not this format. Never carries input text.
#[derive(Debug, PartialEq, Eq)]
pub struct Reject(pub &'static str);

/// The sections systemd knows. A document whose sections all come from this
/// list is a unit file (or a drop-in, or a networkd/nspawn file).
pub const UNIT_SECTIONS: &[&str] = &[
    "Unit",
    "Service",
    "Timer",
    "Socket",
    "Install",
    "Mount",
    "Automount",
    "Path",
    "Slice",
    "Swap",
    "Scope",
    "Container",
    "Network",
    "Match",
    "Link",
    "Address",
    "Route",
    "DHCPv4",
    "DHCPv6",
    "Exec",
    "Files",
    "Resolve",
    "Journal",
    "Login",
    "Manager",
    "Sleep",
    "Coredump",
    "Home",
    "NetDev",
    "Bridge",
    "VLAN",
    "WireGuard",
    "WireGuardPeer",
    "Distribution",
    "Output",
    "Content",
    "Partition",
    // Quadlet and the rest of systemd's families (review 2026-10-23).
    "Pod",
    "Volume",
    "Image",
    "Kube",
    "Build",
    "Quadlet",
    "OOM",
    "Time",
    "PStore",
    "IPv6AcceptRA",
    "DHCPServer",
    "Peer",
];

/// A section counts as systemd's when it is in the list or vendor-prefixed.
fn unit_section(name: &str) -> bool {
    UNIT_SECTIONS.contains(&name) || name.starts_with("X-")
}

enum Line<'a> {
    Section(&'a str),
    Pair(&'a str, &'a str),
    Other,
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('#') || t.starts_with(';')
}

/// Joins physical lines into logical ones: a trailing `\` continues on the
/// next line (systemd and many INI dialects). Comments and blank lines are
/// dropped here; they are counted elsewhere, over the raw input.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut pending: Option<String> = None;
    for raw in text.lines() {
        if pending.is_none() && (raw.trim().is_empty() || is_comment(raw)) {
            continue;
        }
        let (body, continues) = match raw.strip_suffix('\\') {
            Some(b) => (b, true),
            None => (raw, false),
        };
        let mut cur = pending.take().unwrap_or_default();
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(body.trim());
        if continues {
            pending = Some(cur);
        } else {
            out.push(cur);
        }
    }
    if let Some(p) = pending {
        out.push(p);
    }
    out
}

fn classify_line(line: &str, allow_colon: bool) -> Line<'_> {
    let t = line.trim();
    if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Line::Section(inner.trim());
    }
    let eq = t.find('=');
    let colon = if allow_colon { t.find(':') } else { None };
    let split = match (eq, colon) {
        (Some(e), Some(c)) => Some(e.min(c)),
        (Some(e), None) => Some(e),
        (None, Some(c)) => Some(c),
        (None, None) => None,
    };
    match split {
        Some(i) if i > 0 => Line::Pair(t[..i].trim(), t[i + 1..].trim()),
        _ => Line::Other,
    }
}

fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

fn is_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Reads the document, or says why it is none of the three dialects.
/// `forced` names the dialect `--format` asked for: then detection is
/// off, and only a structural contradiction (sections in `.env`, no
/// section in INI/unit) rejects.
pub fn read(text: &str, forced: Option<Dialect>) -> Result<(Dialect, Value), Reject> {
    let lines = logical_lines(text);
    if lines.is_empty() {
        return Err(Reject("empty"));
    }
    let has_section = lines
        .iter()
        .any(|l| matches!(classify_line(l, true), Line::Section(_)));
    match (forced, has_section) {
        (Some(Dialect::Env), true) => return Err(Reject("sections in a .env file")),
        (Some(Dialect::Env), false) | (None, false) => return read_env(&lines),
        (Some(_), false) => return Err(Reject("no section")),
        _ => {}
    }
    // Sections first, keys per section, in document order.
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut current: Option<usize> = None;
    for l in &lines {
        match classify_line(l, true) {
            Line::Section(name) => {
                let idx = match sections.iter().position(|(n, _)| n == name) {
                    Some(i) => i,
                    None => {
                        sections.push((name.to_owned(), Vec::new()));
                        sections.len() - 1
                    }
                };
                current = Some(idx);
            }
            Line::Pair(k, v) => {
                let Some(i) = current else {
                    return Err(Reject("key before first section"));
                };
                sections[i].1.push((k.to_owned(), unquote(v).to_owned()));
            }
            Line::Other => return Err(Reject("line without key=value")),
        }
    }
    // Unit, as soon as ONE section is systemd's: a Quadlet `[Pod]` or an
    // `[X-…]` beside `[Service]` must not turn the file into INI with
    // scalar, silently overwritten keys (review 2026-10-23).
    let unit = match forced {
        Some(Dialect::Unit) => true,
        Some(Dialect::Ini) => false,
        _ => sections.iter().any(|(n, _)| unit_section(n)),
    };
    let mut root = Map::new();
    for (name, pairs) in sections {
        let mut obj = Map::new();
        for (k, v) in pairs {
            if unit {
                match obj.get_mut(&k) {
                    Some(Value::Array(a)) => a.push(Value::String(v)),
                    _ => {
                        obj.insert(k, Value::Array(vec![Value::String(v)]));
                    }
                }
            } else {
                obj.insert(k, Value::String(v));
            }
        }
        root.insert(name, Value::Object(obj));
    }
    Ok((
        if unit { Dialect::Unit } else { Dialect::Ini },
        Value::Object(root),
    ))
}

fn read_env(lines: &[String]) -> Result<(Dialect, Value), Reject> {
    let mut root = Map::new();
    for l in lines {
        let t = l
            .strip_prefix("export ")
            .map_or(l.as_str(), str::trim_start);
        // Only `=` splits a .env line: `KEY: value` is not .env, and a `:`
        // inside the value (a URL) must stay in the value.
        let Line::Pair(k, v) = classify_line(t, false) else {
            return Err(Reject("not key=value lines"));
        };
        if !is_env_key(k) {
            return Err(Reject("not key=value lines"));
        }
        root.insert(k.to_owned(), Value::String(unquote(v).to_owned()));
    }
    Ok((Dialect::Env, Value::Object(root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_keys_are_arrays_and_values_hidden_in_tree() {
        let (d, v) = read(
            "[Unit]\nDescription=x\n[Service]\nExecStart=\nExecStart=/bin/a Zq7Canary\nEnvironment=A=1\n[Service]\nExecStartPre=/bin/b\n",
            None,
        )
        .unwrap();
        assert_eq!(d, Dialect::Unit);
        assert_eq!(v["Service"]["ExecStart"].as_array().unwrap().len(), 2);
        assert_eq!(v["Service"]["ExecStart"][0], "");
        assert_eq!(v["Service"]["ExecStartPre"][0], "/bin/b");
        assert_eq!(v["Unit"]["Description"][0], "x");
    }

    #[test]
    fn continuation_lines_join() {
        let (_, v) = read("[Service]\nExecStart=/bin/a \\\n  --flag\n", None).unwrap();
        assert_eq!(v["Service"]["ExecStart"][0], "/bin/a --flag");
    }

    #[test]
    fn ini_keys_are_scalars_colon_allowed() {
        let (d, v) = read("; c\n[db]\nhost: h\nport=5\npassword = \"p=q\"\n", None).unwrap();
        assert_eq!(d, Dialect::Ini);
        assert_eq!(v["db"]["host"], "h");
        assert_eq!(v["db"]["password"], "p=q");
    }

    #[test]
    fn env_value_is_everything_after_first_equals() {
        let (d, v) = read("# c\nexport KEY=\"a=b=c\"\nTOKEN=abc==\n", None).unwrap();
        assert_eq!(d, Dialect::Env);
        assert_eq!(v["KEY"], "a=b=c");
        assert_eq!(v["TOKEN"], "abc==");
    }

    #[test]
    fn prose_is_rejected() {
        assert!(read("Hello world\nthis is text\n", None).is_err());
        assert!(read("[Section]\nnot a pair\n", None).is_err());
        assert!(read("\n\n", None).is_err());
    }

    #[test]
    fn reject_carries_no_input() {
        let e = read("Zq7Canary line\n", None).unwrap_err();
        assert!(!e.0.contains("Zq7"));
    }
}
