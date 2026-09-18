//! The promise is negative — a value does NOT appear — so every test here
//! plants a canary and looks for it in everything the binary wrote.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const CANARY: &str = "Zq7Canary9xW3kP5mR2t";

fn run(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gestalt"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn text(o: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn assert_no_canary(o: &Output) {
    let (out, err) = text(o);
    assert!(
        !out.contains(CANARY) && !err.contains("Canary"),
        "canary leaked:\n{out}\n{err}"
    );
}

#[test]
fn no_string_value_is_printed() {
    let input = format!(
        r#"{{"apiKey": "{CANARY}", "trackers": ["https://t.example/announce/{CANARY}"],
            "nested": {{"deep": [{{"token": "{CANARY}", "note": "free text {CANARY}"}}]}}}}"#
    );
    let o = run(&[], &input);
    assert!(o.status.success());
    assert_no_canary(&o);
    let (out, _) = text(&o);
    assert!(out.contains(".apiKey"), "{out}");
    assert!(out.contains("token"), "{out}");
    assert!(out.contains(".trackers[]") && out.contains("url"), "{out}");
}

#[test]
fn data_keys_are_not_printed() {
    let input = format!(
        r#"{{"sessions": {{"{CANARY}": {{"user": "a"}}, "user@example.org": {{"user": "b"}}}}}}"#
    );
    let o = run(&[], &input);
    assert_no_canary(&o);
    let (out, _) = text(&o);
    assert!(!out.contains("user@example.org"), "{out}");
    assert!(out.contains(".sessions.{*}.user"), "{out}");
}

#[test]
fn numbers_hidden_unless_asked() {
    let o = run(&[], r#"{"pin": 482913}"#);
    let (out, _) = text(&o);
    assert!(
        !out.contains("482913") && out.contains(".pin  int"),
        "{out}"
    );
    let (out, _) = text(&run(&["--numbers"], r#"{"pin": 482913}"#));
    assert!(out.contains("= 482913"), "{out}");
}

#[test]
fn booleans_and_nulls_are_shown() {
    let (out, _) = text(&run(&[], r#"{"allowPasswordLogin": false, "x": null}"#));
    assert!(out.contains(".allowPasswordLogin  bool = false"), "{out}");
    assert!(out.contains(".x                   null"), "{out}");
}

#[test]
fn show_prints_exactly_the_named_path() {
    let input = format!(r#"{{"version": "2.1.0", "apiKey": "{CANARY}"}}"#);
    let o = run(&["--show", ".version"], &input);
    assert!(o.status.success());
    assert_no_canary(&o);
    let (out, _) = text(&o);
    assert!(out.contains(r#"= "2.1.0""#), "{out}");
}

#[test]
fn show_across_array_elements() {
    let input = r#"[{"name": "a"}, {"name": "b"}, {"name": "a"}]"#;
    let (out, _) = text(&run(&["-s", ".[].name"], input));
    assert!(out.contains(r#"= "a"×2, "b""#), "{out}");
}

#[test]
fn unmatched_show_is_loud() {
    let o = run(&["--show", ".versoin"], r#"{"version": "1"}"#);
    assert_eq!(o.status.code(), Some(3));
    let (out, err) = text(&o);
    assert!(err.contains("--show .versoin matched nothing"), "{err}");
    assert!(
        out.contains(".version"),
        "the description is still printed: {out}"
    );
}

#[test]
fn show_on_a_container_is_refused() {
    let input = format!(r#"{{"cfg": {{"key": "{CANARY}"}}}}"#);
    let o = run(&["--show", ".cfg"], &input);
    assert_eq!(o.status.code(), Some(3));
    assert_no_canary(&o);
}

#[test]
fn not_json_is_characterised_not_echoed() {
    let o = run(
        &[],
        &format!("<html><body>401 Unauthorized {CANARY}</body></html>"),
    );
    assert_eq!(o.status.code(), Some(1));
    assert_no_canary(&o);
    let (_, err) = text(&o);
    assert!(err.contains("not JSON") && err.contains("HTML"), "{err}");
}

#[test]
fn broken_json_after_a_secret_is_not_echoed() {
    let o = run(&[], &format!(r#"{{"k": "{CANARY}", oops}}"#));
    assert_eq!(o.status.code(), Some(1));
    assert_no_canary(&o);
}

#[test]
fn empty_input() {
    let o = run(&[], "  \n");
    assert_eq!(o.status.code(), Some(1));
    assert!(text(&o).1.contains("empty"));
}

#[test]
fn json_lines_are_merged() {
    let (out, _) = text(&run(&[], "{\"a\": 1}\n{\"a\": 2, \"b\": true}\n"));
    assert!(out.starts_with("# 2 documents"), "{out}");
    assert!(out.contains("(in 1 of 2)"), "{out}");
}

#[test]
fn control_characters_in_keys_are_escaped() {
    let (out, _) = text(&run(&[], "{\"a\\u001b[31mb\": 1}"));
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(out.contains(".\"a\\u001b[31mb\""), "{out}");
}

#[test]
fn file_argument() {
    let dir = std::env::temp_dir().join(format!("gestalt-test-{}", std::process::id()));
    std::fs::write(&dir, r#"{"a": 1}"#).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_gestalt"))
        .arg(&dir)
        .output()
        .unwrap();
    std::fs::remove_file(&dir).unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains(".a  int"));
}
