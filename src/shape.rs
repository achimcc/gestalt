//! Walks a JSON value and records, per path, what occurred there.
//!
//! A path is written the way it is printed: `.items[].name`, `.users.{*}.id`,
//! `."key with spaces"`. The same string is what `--show` takes, so a path
//! can be copied from the output into the next call.

use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;
use serde_json::Value;

use crate::classify::{Class, classify};

/// How many distinct values a shown path keeps. Beyond that it only counts.
pub const VALUE_LIMIT: usize = 20;

#[derive(Debug, Default)]
pub struct Stats {
    /// How often this path was reached.
    pub n: usize,
    /// The enclosing object's path, when this path is one of its named keys.
    /// Used for "present in 40 of 42".
    pub named_in: Option<String>,
    /// Reached through data keys (`{*}`) rather than a field name.
    pub via_data_key: bool,

    pub nulls: usize,
    pub bools: usize,
    pub ints: usize,
    pub floats: usize,
    pub strings: usize,
    pub str_len: Option<(usize, usize)>,
    pub classes: BTreeMap<Class, usize>,
    pub arrays: usize,
    pub arr_len: Option<(usize, usize)>,
    pub objects: usize,
    pub obj_keys: Option<(usize, usize)>,

    /// Distinct values (as JSON text) with their counts — only filled for
    /// booleans, for `--show` paths and, with `--numbers`, for numbers.
    pub values: Vec<(String, usize)>,
    /// Occurrences of values that did not fit into `values`.
    pub values_more: usize,
}

impl Stats {
    fn add_value(&mut self, v: &Value) {
        let text = v.to_string();
        if let Some(e) = self.values.iter_mut().find(|(t, _)| *t == text) {
            e.1 += 1;
        } else if self.values.len() < VALUE_LIMIT {
            self.values.push((text, 1));
        } else {
            self.values_more += 1;
        }
    }

    pub fn only_containers(&self) -> bool {
        self.nulls + self.bools + self.ints + self.floats + self.strings == 0
    }
}

fn widen(r: &mut Option<(usize, usize)>, x: usize) {
    *r = Some(match *r {
        None => (x, x),
        Some((lo, hi)) => (lo.min(x), hi.max(x)),
    });
}

pub struct Collector<'a> {
    pub paths: IndexMap<String, Stats>,
    pub documents: usize,
    show: &'a HashSet<String>,
    numbers: bool,
}

impl<'a> Collector<'a> {
    pub fn new(show: &'a HashSet<String>, numbers: bool) -> Self {
        Collector {
            paths: IndexMap::new(),
            documents: 0,
            show,
            numbers,
        }
    }

    pub fn add_document(&mut self, v: &Value) {
        self.documents += 1;
        self.visit(v, ".", None, false);
    }

    fn visit(&mut self, v: &Value, path: &str, named_in: Option<&str>, via_data_key: bool) {
        let shown = self.show.contains(path);
        let numbers = self.numbers;
        let st = self.paths.entry(path.to_owned()).or_insert_with(|| Stats {
            named_in: named_in.map(str::to_owned),
            via_data_key,
            ..Stats::default()
        });
        st.n += 1;
        match v {
            Value::Null => st.nulls += 1,
            Value::Bool(_) => {
                st.bools += 1;
                st.add_value(v);
            }
            Value::Number(num) => {
                if num.is_f64() {
                    st.floats += 1;
                } else {
                    st.ints += 1;
                }
                if shown || numbers {
                    st.add_value(v);
                }
            }
            Value::String(s) => {
                st.strings += 1;
                widen(&mut st.str_len, s.chars().count());
                *st.classes.entry(classify(s)).or_default() += 1;
                if shown {
                    st.add_value(v);
                }
            }
            Value::Array(items) => {
                st.arrays += 1;
                widen(&mut st.arr_len, items.len());
                let child = child_path(path, "[]");
                for item in items {
                    self.visit(item, &child, None, false);
                }
            }
            Value::Object(map) => {
                st.objects += 1;
                widen(&mut st.obj_keys, map.len());
                for (key, value) in map {
                    if classify(key).is_data_like() {
                        self.visit(value, &child_path(path, ".{*}"), None, true);
                    } else {
                        let child = child_path(path, &format!(".{}", key_segment(key)));
                        self.visit(value, &child, Some(path), false);
                    }
                }
            }
        }
    }
}

fn child_path(parent: &str, segment: &str) -> String {
    if parent == "." {
        // `.` + `.name` would read `..name`; `.` + `[]` would read `.[]`,
        // which is jq's spelling and fine.
        match segment.strip_prefix('.') {
            Some(rest) => format!(".{rest}"),
            None => format!(".{segment}"),
        }
    } else {
        format!("{parent}{segment}")
    }
}

/// A field name as it appears in a path: bare when it is a plain identifier,
/// JSON-quoted otherwise — which also escapes control characters, so a key
/// cannot write escape sequences to the terminal.
pub fn key_segment(key: &str) -> String {
    let mut chars = key.chars();
    let plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if plain {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn collect(v: Value) -> IndexMap<String, Stats> {
        let show = HashSet::new();
        let mut c = Collector::new(&show, false);
        c.add_document(&v);
        c.paths
    }

    #[test]
    fn paths() {
        let p = collect(json!({"a": {"b": [1, {"c": null}]}, "x y": true}));
        let keys: Vec<_> = p.keys().map(String::as_str).collect();
        assert_eq!(keys, [".", ".a", ".a.b", ".a.b[]", ".a.b[].c", ".\"x y\""]);
    }

    #[test]
    fn root_array() {
        let p = collect(json!([{"id": 1}, {"id": 2, "extra": "x"}]));
        assert_eq!(p[".[]"].objects, 2);
        assert_eq!(p[".[].extra"].n, 1);
        assert_eq!(p[".[].extra"].named_in.as_deref(), Some(".[]"));
    }

    #[test]
    fn data_keys_fold() {
        let p = collect(json!({"users": {"1001": {"name": "a"}, "1002": {"name": "b"}}}));
        assert!(p.contains_key(".users.{*}.name"));
        assert!(p.keys().all(|k| !k.contains("1001")));
    }

    #[test]
    fn strings_are_not_kept() {
        let p = collect(json!({"k": "secret-value-123456789"}));
        assert!(p[".k"].values.is_empty());
    }
}
