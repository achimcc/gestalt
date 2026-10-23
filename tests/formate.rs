//! The formats besides JSON. Same promise as in cli.rs: a planted value
//! never appears in anything the binary writes.

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
fn unit_file_shape_without_values() {
    let o = run(
        &[],
        &format!(
            "# hidden {CANARY}\n[Service]\nExecStart=/bin/x {CANARY}\nEnvironment=TOKEN={CANARY}\n"
        ),
    );
    assert!(o.status.success(), "{}", text(&o).1);
    assert_no_canary(&o);
    let (out, _) = text(&o);
    assert!(out.starts_with("# 1 document (unit),"), "{out}");
    assert!(out.contains("1 comment line not shown"), "{out}");
    assert!(
        out.contains(".Service.ExecStart[]") && out.contains(".Service.Environment[]"),
        "{out}"
    );
}

#[test]
fn show_on_unit_array_path_works_and_token_is_refused() {
    let input = format!("[Service]\nExecStart=/bin/x\nEnvironment=TOKEN={CANARY}\n");
    let o = run(&["--show", ".Service.ExecStart[]"], &input);
    assert!(o.status.success());
    assert!(text(&o).0.contains("/bin/x"));
    let o = run(&["--show", ".Service.Environment[]"], &input);
    assert_eq!(o.status.code(), Some(4));
    assert_no_canary(&o);
}

#[test]
fn env_file_and_hash() {
    let a = format!("export API_KEY={CANARY}\nPORT=8080\n");
    let o = run(&[], &a);
    assert!(o.status.success(), "{}", text(&o).1);
    assert_no_canary(&o);
    assert!(text(&o).0.contains("(env)"), "{}", text(&o).0);
    let o = run(&["--hash", ".API_KEY"], &a);
    assert!(o.status.success());
    assert_no_canary(&o);
    assert!(text(&o).0.contains("sha256:"));
}

#[test]
fn toml_and_yaml_are_told_apart() {
    let o = run(
        &[],
        &format!("[owner]\nname = \"{CANARY}\"\nborn = 1979-05-27\n"),
    );
    assert!(text(&o).0.contains("(toml)"), "{}", text(&o).0);
    assert_no_canary(&o);
    let o = run(
        &[],
        &format!("owner:\n  name: {CANARY}\n  note: |\n    {CANARY}\n"),
    );
    assert!(text(&o).0.contains("(yaml)"), "{}", text(&o).0);
    assert_no_canary(&o);
    assert!(text(&o).0.contains(".owner.note"));
}

#[test]
fn broken_json_never_falls_through() {
    for input in [
        format!(r#"{{"k": "{CANARY}", oops}}"#),
        format!("[1, 2, \"{CANARY}\""),
        format!("{{\"a\": 1}}\n{{\"b\": \"{CANARY}\" oops}}"),
    ] {
        let o = run(&[], &input);
        assert_eq!(o.status.code(), Some(1), "{input}");
        assert_no_canary(&o);
        assert!(text(&o).1.contains("not JSON"), "{}", text(&o).1);
    }
}

#[test]
fn prose_is_characterised_not_read() {
    let o = run(
        &[],
        &format!("Dear reader,\nthe key is {CANARY} and that is all.\n"),
    );
    assert_eq!(o.status.code(), Some(1));
    assert_no_canary(&o);
    assert!(text(&o).1.contains("looks like text"), "{}", text(&o).1);
}

#[test]
fn forced_format_wins_and_fails_loudly() {
    let o = run(
        &["--format", "yaml"],
        &format!("[Service]\nExecStart={CANARY}\n"),
    );
    assert!(text(&o).0.contains("(yaml)") || o.status.code() == Some(1));
    assert_no_canary(&o);
    let o = run(&["--format", "toml"], &format!("a: {CANARY}\n"));
    assert_eq!(o.status.code(), Some(1));
    assert_no_canary(&o);
    assert!(text(&o).1.contains("not toml"), "{}", text(&o).1);
    let o = run(&["--format", "xml"], "{}");
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn json_header_is_unchanged() {
    let (out, _) = text(&run(&[], r#"{"a": 1}"#));
    assert!(out.starts_with("# 1 document, 8 bytes\n"), "{out}");
}

#[test]
fn help_mentions_formats() {
    let (out, _) = text(&run(&["--help"], ""));
    assert!(
        out.contains("--format") && out.contains("YAML") && out.contains("unit"),
        "{out}"
    );
}

/// Review 2026-10-23, Critical 1: a BOM or a comment line before broken
/// JSON must not route it past the `{` guard into YAML.
#[test]
fn bom_or_comment_before_broken_json_stays_not_json() {
    for input in [
        format!("\u{feff}{{\"user\": \"a\", \"{CANARY}\"}}"),
        format!("# note\n{{\"user\": \"a\", \"{CANARY}\"}}"),
    ] {
        let o = run(&[], &input);
        assert_eq!(o.status.code(), Some(1), "{input:?}: {}", text(&o).1);
        assert_no_canary(&o);
        assert!(text(&o).1.contains("not JSON"), "{}", text(&o).1);
    }
    let (out, _) = text(&run(&[], "\u{feff}{\"a\": 1}"));
    assert!(out.starts_with("# 1 document, "), "{out}");
}

/// Review 2026-10-23, Critical 2: the key-name refusal must hold on array
/// paths — in a unit file every key is one.
#[test]
fn secret_key_name_is_refused_on_array_paths() {
    let o = run(
        &["--show", ".Container.Secret[]"],
        &format!("[Container]\nSecret={CANARY}\n"),
    );
    assert_eq!(o.status.code(), Some(4), "{}", text(&o).1);
    assert_no_canary(&o);
    let o = run(
        &["--show", ".apiKeys[]"],
        &format!("{{\"apiKeys\": [\"{CANARY}\"]}}"),
    );
    assert_eq!(o.status.code(), Some(4));
    assert_no_canary(&o);
}

/// Review 2026-10-23, Important 3: `--format` forces the dialect instead
/// of demanding that detection agrees.
#[test]
fn forced_dialect_overrides_detection() {
    let (out, err) = text(&run(&["--format", "unit"], "[X-Custom]\nFoo=bar\n"));
    assert!(out.starts_with("# 1 document (unit)"), "{out}{err}");
    assert!(out.contains(".X-Custom.Foo[]"), "{out}");
    let (out, err) = text(&run(&["--format", "ini"], "[Service]\nExecStart=a\n"));
    assert!(out.starts_with("# 1 document (ini)"), "{out}{err}");
    assert!(out.contains(".Service.ExecStart  "), "{out}");
    let o = run(&["--format", "env"], "[Service]\nExecStart=a\n");
    assert_eq!(o.status.code(), Some(1));
}

/// Review 2026-10-23, Important 4: one unknown section (Quadlet, `X-…`)
/// must not turn the whole file into INI with scalar, overwritten keys.
#[test]
fn unknown_section_keeps_unit_dialect() {
    let (out, _) = text(&run(
        &[],
        "[Service]\nExecStart=a\nExecStart=b\n[X-Custom]\nFoo=bar\n",
    ));
    assert!(out.starts_with("# 1 document (unit)"), "{out}");
    assert!(
        out.contains(".Service.ExecStart[]") && out.contains("array[2]"),
        "{out}"
    );
    let (out, _) = text(&run(&[], "[Pod]\nPodName=x\n[Volume]\nDriver=y\n"));
    assert!(out.starts_with("# 1 document (unit)"), "{out}");
}
