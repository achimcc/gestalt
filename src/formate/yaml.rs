//! YAML into the JSON tree — one value per `---` document.

use serde_json::Value;

use serde::Deserialize as _;
use serde_norway::Value as Y;

use super::ini::Reject;

/// Reads every `---` document. A document that is neither a mapping nor a
/// sequence (prose reads as one YAML scalar) is rejected, and so is a parse
/// error — whose message would quote the input and is therefore dropped.
pub fn read(text: &str) -> Result<Vec<Value>, Reject> {
    let mut out = Vec::new();
    for doc in serde_norway::Deserializer::from_str(text) {
        let v = Y::deserialize(doc).map_err(|_| Reject("not yaml"))?;
        match v {
            Y::Mapping(_) | Y::Sequence(_) => out.push(convert(v)),
            // A trailing `---` gives an empty document; skip it, as JSON
            // lines skip blank lines.
            Y::Null if !out.is_empty() => {}
            _ => return Err(Reject("scalar")),
        }
    }
    if out.is_empty() {
        return Err(Reject("empty"));
    }
    Ok(out)
}

fn convert(v: Y) -> Value {
    match v {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(u) = n.as_u64() {
                Value::from(u)
            } else {
                n.as_f64()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::Null, Value::Number)
            }
        }
        Y::String(s) => Value::String(s),
        Y::Sequence(items) => Value::Array(items.into_iter().map(convert).collect()),
        Y::Mapping(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (key_text(&k), convert(v)))
                .collect(),
        ),
        // `!secret abc`: the tag is dropped, the inner value is described
        // like any other — as a string, so the classifier sees it.
        Y::Tagged(t) => match convert(t.value) {
            Value::String(s) => Value::String(s),
            Value::Null => Value::String(String::new()),
            Value::Bool(b) => Value::String(b.to_string()),
            Value::Number(n) => Value::String(n.to_string()),
            other => other,
        },
    }
}

/// A mapping key that is not a string gets its text form; `is_field_name`
/// then folds a numeric or structured key into `{*}` anyway.
fn key_text(k: &Y) -> String {
    match k {
        Y::String(s) => s.clone(),
        Y::Null => "null".to_owned(),
        Y::Bool(b) => b.to_string(),
        Y::Number(n) => n.to_string(),
        other => serde_norway::to_string(other)
            .map(|s| s.trim().to_owned())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_documents_and_block_scalar() {
        let docs = read("a: 1\nb: |\n  Zq7Canary\n  line2\n---\nc: [x, y]\n").unwrap();
        assert_eq!(docs.len(), 2);
        assert!(docs[0]["b"].as_str().unwrap().contains("Zq7Canary"));
        assert_eq!(docs[1]["c"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn yaml_non_string_keys_and_tags() {
        let docs = read("1: a\n? [x]\n: b\nt: !secret Zq7Canary\n").unwrap();
        let d = &docs[0];
        assert_eq!(d["1"], "a");
        assert_eq!(d["t"], "Zq7Canary");
        assert_eq!(d.as_object().unwrap().len(), 3);
    }

    #[test]
    fn yaml_scalar_document_is_rejected() {
        assert!(read("just a sentence\n").is_err());
        assert!(read("Zq7Canary: a\nnot: [valid\n").is_err());
    }
}
