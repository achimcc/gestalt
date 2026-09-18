//! Turns the collected stats into one line per path.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::shape::Stats;

/// Paths longer than this do not push every other line to the right.
const MAX_PATH_COLUMN: usize = 48;

pub fn render(paths: &IndexMap<String, Stats>, documents: usize, bytes: usize) -> String {
    let mut out = String::new();
    let docs = if documents == 1 {
        "1 document".to_owned()
    } else {
        format!("{documents} documents")
    };
    let _ = writeln!(out, "# {docs}, {bytes} bytes");
    let width = paths
        .keys()
        .map(|p| p.chars().count())
        .max()
        .unwrap_or(0)
        .min(MAX_PATH_COLUMN);
    for (path, st) in paths {
        let mut line = format!("{path:<width$}  {}", describe(st));
        if let Some(presence) = presence(paths, st) {
            line.push_str("  ");
            line.push_str(&presence);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn range(r: Option<(usize, usize)>) -> String {
    match r {
        Some((lo, hi)) if lo == hi => lo.to_string(),
        Some((lo, hi)) => format!("{lo}..{hi}"),
        None => String::new(),
    }
}

fn describe(st: &Stats) -> String {
    let mut kinds: Vec<(String, usize)> = Vec::new();
    if st.objects > 0 {
        kinds.push((format!("object{{{}}}", range(st.obj_keys)), st.objects));
    }
    if st.arrays > 0 {
        kinds.push((format!("array[{}]", range(st.arr_len)), st.arrays));
    }
    if st.strings > 0 {
        let mut classes: Vec<_> = st.classes.iter().collect();
        classes.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let classes = if classes.len() == 1 {
            classes[0].0.name().to_owned()
        } else {
            classes
                .iter()
                .map(|(c, k)| {
                    if **k == 1 {
                        c.name().to_owned()
                    } else {
                        format!("{}×{k}", c.name())
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        kinds.push((
            format!("string({}) {classes}", range(st.str_len)),
            st.strings,
        ));
    }
    let numbers = st.ints + st.floats;
    if numbers > 0 {
        let name = match (st.ints, st.floats) {
            (_, 0) => "int",
            (0, _) => "float",
            _ => "number",
        };
        kinds.push((name.to_owned(), numbers));
    }
    if st.bools > 0 {
        kinds.push(("bool".to_owned(), st.bools));
    }
    if st.nulls > 0 {
        kinds.push(("null".to_owned(), st.nulls));
    }

    let mut s = if kinds.len() == 1 {
        kinds.remove(0).0
    } else {
        kinds
            .iter()
            .map(|(k, n)| format!("{n}× {k}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    if !st.values.is_empty() {
        s.push_str(" = ");
        s.push_str(&values(st));
    }
    s
}

fn values(st: &Stats) -> String {
    let counted = st.values.len() > 1 || st.values_more > 0;
    let mut parts: Vec<String> = st
        .values
        .iter()
        .map(|(v, n)| {
            if counted && *n > 1 {
                format!("{v}×{n}")
            } else {
                v.clone()
            }
        })
        .collect();
    if st.values_more > 0 {
        parts.push(format!("… (+{} more)", st.values_more));
    }
    parts.join(", ")
}

fn presence(paths: &IndexMap<String, Stats>, st: &Stats) -> Option<String> {
    if st.via_data_key {
        return Some(format!("(×{})", st.n));
    }
    let parent = paths.get(st.named_in.as_deref()?)?;
    (st.n < parent.objects).then(|| format!("(in {} of {})", st.n, parent.objects))
}
