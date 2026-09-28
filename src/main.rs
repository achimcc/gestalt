use std::collections::HashSet;
use std::io::{Read, Write};
use std::process::ExitCode;

use gestalt::{Options, describe};

const HELP: &str = "\
gestalt — show the shape of a JSON document without showing its values

USAGE:
    gestalt [OPTIONS] [FILE]
    curl -s … | gestalt [OPTIONS]

Reads FILE, or standard input when FILE is missing or `-`. Several documents
in a row (JSON lines) are described together.

OPTIONS:
    -s, --show PATH   print the values at PATH (a scalar path exactly as
                      gestalt prints it, e.g. `.items[].name`); repeatable.
                      REFUSED (exit 4) where a value looks like a secret:
                      class token, hex, url or uuid, or a key named like
                      *key*, *token*, *pass*, *secret*, *session*, …
        --hash PATH   print SHA-256 of the values at PATH instead — to tell
                      whether two secrets are the same; repeatable
    -n, --numbers     print the values of all numbers
    -h, --help        print this help
    -V, --version     print the version

Strings and numbers are hidden by default; booleans and nulls are shown.
Keys that look like data (ids, tokens, e-mail addresses, …) become `{*}`.

EXIT STATUS:
    0  described
    1  input unreadable, empty or not JSON
    2  usage error
    3  described, but a --show path matched nothing or only objects/arrays
    4  described, but a --show path was refused (a value looked like a secret)
";

struct Args {
    opts: Options,
    file: Option<String>,
}

fn parse_args() -> Result<Option<Args>, lexopt::Error> {
    use lexopt::prelude::*;
    let mut show = HashSet::new();
    let mut hash = HashSet::new();
    let mut numbers = false;
    let mut file = None;
    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('s') | Long("show") => {
                show.insert(parser.value()?.string()?);
            }
            Long("hash") => {
                hash.insert(parser.value()?.string()?);
            }
            Short('n') | Long("numbers") => numbers = true,
            Short('h') | Long("help") => {
                print!("{HELP}");
                return Ok(None);
            }
            Short('V') | Long("version") => {
                println!("gestalt {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            Value(v) if file.is_none() => file = Some(v.string()?),
            _ => return Err(arg.unexpected()),
        }
    }
    Ok(Some(Args {
        opts: Options {
            show,
            hash,
            numbers,
        },
        file,
    }))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gestalt: {e}\nTry `gestalt --help`.");
            return ExitCode::from(2);
        }
    };

    let mut input = Vec::new();
    let read = match args.file.as_deref() {
        None | Some("-") => std::io::stdin().read_to_end(&mut input),
        Some(path) => std::fs::File::open(path).and_then(|mut f| f.read_to_end(&mut input)),
    };
    if let Err(e) = read {
        eprintln!("gestalt: cannot read input: {e}");
        return ExitCode::from(1);
    }

    let report = match describe(&input, &args.opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("gestalt: {e}");
            return ExitCode::from(1);
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(report.text.as_bytes())
        .and_then(|_| stdout.flush())
        .is_err()
    {
        // A closed pipe (`| head`) is not an error worth a message.
        return ExitCode::SUCCESS;
    }

    let mut status = ExitCode::SUCCESS;
    for p in &report.unmatched {
        eprintln!("gestalt: --show {p} matched nothing — copy the path from the output above");
        status = ExitCode::from(3);
    }
    for p in &report.containers {
        eprintln!("gestalt: --show {p} holds only objects/arrays — show a scalar path below it");
        status = ExitCode::from(3);
    }
    // Last, so it wins over 3: a refused path is the answer that matters.
    for p in &report.refused {
        eprintln!(
            "gestalt: --show {p} refused: a value there looks like a secret — to compare, use --hash {p}"
        );
        status = ExitCode::from(4);
    }
    status
}
