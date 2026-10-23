//! TOML into the JSON tree.

use serde_json::Value;

use super::ini::Reject;

/// Reads one TOML document. The parser's error is not passed on: it quotes
/// the input.
pub fn read(text: &str) -> Result<Value, Reject> {
    let table: toml::Table = toml::from_str(text).map_err(|_| Reject("not toml"))?;
    Ok(convert(toml::Value::Table(table)))
}

fn convert(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        toml::Value::Boolean(b) => Value::Bool(b),
        // A date is a string to the classifier (`date`/`datetime`), not a
        // private struct that would print as an object.
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(convert).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, convert(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_tree_and_datetime_as_string() {
        let v = read("title = \"Zq7Canary\"\n[owner]\nborn = 1979-05-27\nids = [1, 2]\n").unwrap();
        assert_eq!(v["owner"]["born"], "1979-05-27");
        assert_eq!(v["owner"]["ids"].as_array().unwrap().len(), 2);
        assert_eq!(v["title"], "Zq7Canary");
    }

    #[test]
    fn toml_multiline_string_is_one_value() {
        let v = read("k = \"\"\"\nZq7Canary\nmore\n\"\"\"\n").unwrap();
        assert!(v["k"].as_str().unwrap().contains("Zq7Canary"));
    }

    #[test]
    fn toml_reject_has_no_input() {
        let e = read("k = Zq7Canary\n").unwrap_err();
        assert!(!e.0.contains("Zq7"));
    }
}
