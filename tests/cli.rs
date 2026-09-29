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
fn control_characters_in_keys_do_not_reach_the_terminal() {
    // Since 0.3.0 (B135) such a key is not a field name at all and folds.
    let (out, _) = text(&run(&[], "{\"a\\u001b[31mb\": 1}"));
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(!out.contains("[31mb"), "{out}");
    assert!(out.contains(".{*}  int"), "{out}");
}

/// B135 (homeserver audit 3, CD-6): serde_json escapes only U+0000..U+001F.
/// DEL, the 8-bit CSI U+009B and the bidi override U+202E went to the
/// terminal raw — in keys and in `--show` values. The audit's own input.
const TERMINAL_CHARS: [char; 7] = [
    '\u{7f}', '\u{9b}', '\u{85}', '\u{202e}', '\u{2066}', '\u{200f}', '\u{2028}',
];

fn assert_no_terminal_chars(o: &Output) {
    for stream in [&o.stdout, &o.stderr] {
        let s = String::from_utf8_lossy(stream);
        for c in TERMINAL_CHARS {
            assert!(!s.contains(c), "raw U+{:04X} in output: {s:?}", c as u32);
        }
    }
}

#[test]
fn c1_del_and_bidi_in_keys_are_not_printed_raw() {
    let o = run(
        &[],
        "{\"a\u{9b}31mX\":1,\"b\u{202e}evil\":2,\"c\u{7f}d\":3}",
    );
    assert!(o.status.success());
    assert_no_terminal_chars(&o);
}

#[test]
fn c1_del_and_bidi_in_shown_values_are_escaped() {
    let o = run(
        &["--show", ".note"],
        "{\"note\": \"a\u{9b}31mX b\u{202e}evil c\u{7f}d e\u{85}f g\u{2066}h\u{200f}i\u{2028}j\"}",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_no_terminal_chars(&o);
    let (out, _) = text(&o);
    for esc in [
        "\\u009b", "\\u202e", "\\u007f", "\\u0085", "\\u2066", "\\u200f", "\\u2028",
    ] {
        assert!(out.contains(esc), "{esc} missing: {out}");
    }
    // The visible rest is still there — escaped, not dropped.
    assert!(out.contains("31mX b"), "{out}");
}

/// B134 (homeserver audit 3, CD-5): "when in doubt, fold". Keys that are
/// credentials in practice but did not look like a token to the classifier
/// were printed as field names. The audit's corpus (gs2_schluessel_korpus).
#[test]
fn secret_like_map_keys_fold() {
    let keys = [
        "Qx7mK2pR9sT4vW8z",                          // token16_alnum
        "Qx7mK2pR9sT4vW8",                           // token15_alnum
        "QxmKpRsTvWzaBcDeFgHj",                      // token, letters only
        "Qx7!mK2pR9sT4vW8zQ",                        // password with special characters
        "admin:Qx7mK2pR9sT",                         // basic auth user:pass
        "Bearer Qx7mK2pR9sT4vW8z",                   // authorization header
        "Qx7mK2pR%2BsT4vW8z",                        // percent-encoded
        "achim@localhost",                           // e-mail without a dot in the domain
        "someone@example.org",                       // e-mail
        "secrets/Qx7mK2pR9s.key",                    // relative path
        "/run/secrets/Qx7mK2pR9s",                   // absolute path
        "a1b2c3d",                                   // hex, 7
        "a1b2c3d4",                                  // hex, 8
        "server.taile9e283.ts.net",                  // host name
        "eyJhbGciOiJIUzI1NiJ9",                      // part of a JWT
        "482913",                                    // TOTP
        "aa:bb:cc:dd:ee:ff",                         // MAC address
        "Jürgen Müller",                             // a person's name
        "passkey=Qx7mK2pR9sT4",                      // announce passkey
        "x7Kq2mZp9",                                 // short, mixed case and digits
        "abcdefghijklmnopqrstuvwxyzabcdefghijklmno", // longer than 40
    ];
    for k in keys {
        let input = serde_json::json!({"cfg": {"name": "a", k: {"user": "x"}}}).to_string();
        let o = run(&[], &input);
        assert!(o.status.success(), "{k}");
        let (out, err) = text(&o);
        assert!(
            !out.contains(k) && !err.contains(k),
            "{k:?} printed:\n{out}"
        );
        assert!(out.contains(".cfg.{*}.user"), "{k:?} not folded:\n{out}");
        assert!(out.contains(".cfg.name"), "{out}");
    }
}

/// The other side of B134: folding everything would make gestalt useless.
/// Field names as real APIs spell them stay readable.
#[test]
fn field_names_stay_readable() {
    let names = [
        "name",
        "apiKey",
        "api_key",
        "x-forwarded-for",
        "Version",
        "v2",
        "ipv4",
        "sha256",
        "x509Certificate",
        "oauth2Enabled",
        "ec2InstanceId",
        "address_line_2",
        "getElementsByTagName",
        "XMLHttpRequest",
        "ENABLE_ADDITIONAL_METRICS",
        "__name__",
        "userId",
        "isAdmin",
        "ID",
    ];
    for k in names {
        let input = serde_json::json!({ k: true }).to_string();
        let (out, _) = text(&run(&[], &input));
        assert!(
            out.contains(&format!(".{k}  bool = true")),
            "{k} folded:\n{out}"
        );
    }
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

/// B91 (homeserver audit 3): `--show` on a secret-looking value is refused,
/// exit 4, and the value appears nowhere — by class and by key name.
#[test]
fn show_refuses_what_looks_like_a_secret() {
    let input = format!(
        r#"{{"apiKey": "{CANARY}", "password": "short pw {CANARY}", "url": "https://x.example/p?k={CANARY}", "name": "plain name"}}"#
    );
    for p in [".apiKey", ".password", ".url"] {
        let o = run(&["--show", p], &input);
        assert_eq!(o.status.code(), Some(4), "{p}");
        assert_no_canary(&o);
        let (_, err) = text(&o);
        assert!(err.contains("refused") && err.contains("--hash"), "{err}");
    }
    let o = run(&["--show", ".name"], &input);
    assert_eq!(o.status.code(), Some(0));
    assert!(text(&o).0.contains("plain name"));
}

/// `--hash` answers "is it the same secret?" without the secret.
#[test]
fn hash_compares_without_showing() {
    let a = format!(r#"{{"apiKey": "{CANARY}"}}"#);
    let b = format!(r#"{{"apiKey": "{CANARY}x"}}"#);
    let oa = run(&["--hash", ".apiKey"], &a);
    let oa2 = run(&["--hash", ".apiKey"], &a);
    let ob = run(&["--hash", ".apiKey"], &b);
    assert!(oa.status.success());
    assert_no_canary(&oa);
    let h = |o: &Output| {
        text(o)
            .0
            .split("sha256:")
            .nth(1)
            .map(|s| s[..64].to_owned())
            .expect("a sha256 in the output")
    };
    assert_eq!(h(&oa), h(&oa2));
    assert_ne!(h(&oa), h(&ob));
}
