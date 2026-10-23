# gestalt

Shows the shape of a document without showing its values — JSON, YAML, TOML,
INI, `.env` and systemd unit files.

The example below is [`examples/downloadclient.json`](examples/downloadclient.json),
shaped like the answer of a Radarr download-client endpoint; the password
and the API key in it are not shown.

```console
$ gestalt examples/downloadclient.json
# 1 document, 753 bytes
.                     array[2]
.[]                   object{7}
.[].enable            bool = true, false
.[].protocol          string(6..7) text
.[].priority          int
.[].name              string(7..11) text
.[].implementation    string(7..11) text
.[].id                int
.[].fields            array[4..5]
.[].fields[]          object{2..3}
.[].fields[].name     string(4..8) text
.[].fields[].value    5× string(3..32) ip×2 hex token text | 2× int | 2× bool = false
.[].fields[].privacy  string(6..8) text  (in 2 of 9)
```

Strings and numbers are replaced by their type, their length and a coarse
class — `token`, `hex`, `uuid`, `ip`, `url`, `email`, `path`, `date`, `datetime`,
`digits`, `empty` or `text`. Booleans and nulls are shown: a bit is not a
secret, and `"allowPasswordLogin": false` is often exactly what you came for.

A value appears only when you name its path:

```console
$ gestalt --show .[].name examples/downloadclient.json
…
.[].name              string(7..11) text = "qBittorrent", "SABnzbd"
…
```

## Why

Exploring an unfamiliar API is when secrets leak. You do not yet know which
field holds the key — that is why you are exploring — and the response that
"only has settings in it" turns out to carry the API keys of three other
services, or a tracker URL with a passkey in its query string. A regex mask
after the pipe only catches what its author thought of, and it is the step
you skip when the output looks harmless.

gestalt inverts the default: nothing is shown unless asked for, and asking
names the exact path. It was written for a homelab where keys had ended up
in a chat log several times — three of them during exploration, each
followed by a rotation.

## What it hides and how

| In the input | In the output |
|---|---|
| string | `string(len)` and its class — never the text |
| number | `int` / `float` — the value only with `--numbers` or `--show` |
| boolean, null | shown |
| object key that is a field name (`apiKey`, `x-request-id`, `ipv4`) | shown |
| any other object key (an id, a token, a hash, an IP address, an e-mail address, a URL, a path, a date, `Bearer …`, `user:pass`, a key with spaces or dots) | folded into `{*}`; its values are described together |
| control, bidi and zero-width characters in a `--show` value | escaped as `\uXXXX`, so a value cannot write to your terminal |
| comment lines (`#`, `;`) in YAML, TOML, INI, `.env`, unit files | counted in the header (`3 comment lines not shown`), never shown — a comment carries a password as easily as a value |
| input in no known format | characterised (`HTML or XML`, `text`, `key=value lines`, `binary`, `truncated or broken JSON`), never echoed |

A key counts as a field name only when it *looks like* one: an identifier
of at most 40 characters (`[A-Za-z_][A-Za-z0-9_-]*`) that no class claims,
with at most two runs of digits (`x509Certificate`, not `a1b2c3d`) and at
most two one- or two-letter camel-case words (`getElementsByTagName`, not
`QxmKpRsTvWza…`). Everything else folds. Until 0.2.0 it was the other way
round — a key was shown unless it looked like data — and a 15-character
token, `Bearer …` or `name@localhost` as a map key went through. When in
doubt, gestalt folds: a folded field name costs readability, a printed
secret costs a rotation.

Arrays are described as one element: `.items[]` merges every element, and
`(in 40 of 42)` says that a field is missing from some of them. Several
documents in a row (JSON lines, the concatenated output of a loop) are
merged the same way.

`--show` takes a path exactly as gestalt prints it, so you copy it from the
previous output. A path that matches nothing is an error (exit status 3) and
not an empty result, because a filter with a typo looks exactly like a field
that has no value. `--show` on an object or an array is refused, because
it would print everything below it — name the scalar path you need.

`--show` also refuses a path where a value looks like a secret (exit
status 4): class `token`, `hex`, `url` or `uuid`, or a key whose name
contains `key`, `token`, `pass`, `secret`, `session`, `cookie`, `credential`,
`authorization`, `signature`, `private`, `announce` or `tracker`. Until 0.2.0
that was a rule for the reader; a 20-character password is plain `text` to
the classifier, so the key name counts as well. To tell whether two secrets
are the same, use `--hash PATH`: it prints the SHA-256 of each value and
nothing else.

## Formats

Since 0.4.0 the format is told from the content, never from a file name
(stdin from `ssh host cat /etc/foo.yaml` has none). The order is JSON,
unit file, TOML, INI, `.env`, YAML; the first reader that takes the whole
input wins, and `--format` forces one. Three guards keep broken JSON from
falling through to a more forgiving reader: input starting with `{` is JSON
or nothing (`{"k": 1, oops}` is a valid YAML flow mapping), input starting
with `[` never reaches YAML, and once JSON has read one document and then
fails, it stays a JSON error. The first significant byte is read after a
BOM, blank lines and comment lines, so `# note` above broken JSON changes
nothing (0.4.1). YAML only counts when the document is a
mapping or a sequence — prose is one YAML scalar and is rejected instead.

Every format is mapped onto the same tree, so paths, classes, `--show`,
`--hash` and the refusal of secret-looking values work alike:

| Format | Mapping |
|---|---|
| YAML | mappings and sequences as objects and arrays; several `---` documents merged like JSON lines; a non-string key gets its text form and then folds; a tag (`!secret x`) is dropped and its value described as a string |
| TOML | tables and arrays; dates and times as strings (class `date`/`datetime`) |
| unit file | `[Section]` as object; **every key is an array** (`.Service.ExecStart[]`), because systemd lets any setting repeat and the path must not depend on how often; `\` continuation lines joined; a file counts as a unit file when every section is one systemd knows |
| INI | `[Section]` as object, scalar keys, `=` or `:` as separator, a repeated key overwrites |
| `.env` | `KEY=value` lines, optional `export`; the value is everything after the first `=` |

The YAML reader is `serde_norway`, which sits on a C-to-Rust translation
of libyaml with plenty of `unsafe` inside; gestalt's own code forbids
`unsafe`, and `cargo audit` runs against the pinned advisory database on
every build. The INI/unit/`.env` reader is gestalt's own, because no crate
reads all three the way they are written in practice.

## Usage

```
gestalt [OPTIONS] [FILE]

  -f, --format FMT  read as json, yaml, toml, ini, env or unit instead of detecting
  -s, --show PATH   print the values at PATH; repeatable; refused for secrets
      --hash PATH   print SHA-256 of the values at PATH; repeatable
  -n, --numbers     print the values of all numbers
  -h, --help
  -V, --version
```

Exit status: `0` described, `1` input unreadable, empty or in no known format, `2`
usage error, `3` described, but a `--show` path matched nothing or only
objects/arrays, `4` described, but a `--show` path was refused.

## Keeping the key out of argv as well

gestalt only reads. The other half of a leak is the request: a key given as
`curl -H 'X-Api-Key: …'` is in `/proc/<pid>/cmdline`, in your shell history
and — when run through `systemd-run` — in the journal. Pass headers from a
file instead:

```sh
curl -s -H @header.txt https://…  | gestalt
# or build it on the fly, with the key in no process's argv:
printf 'header = "X-Api-Key: %s"\n' "$(cat /run/credentials/api-key)" | curl -s -K - https://… | gestalt
```

## Limits

- The classes are heuristics. A short secret (`hunter2`) is `text`, not
  `token` — but it is hidden all the same; the class never contains the value.
- Field names are shown. A secret used as a map key is folded unless it
  looks like a field name — a short word-like one (`hunter2`, a lower-case
  passphrase without digits) still does.
- The whole input is read into memory.
- A `.env` file made only of numbers and booleans (`PORT=8080`) is valid
  TOML and is read as such; the shape is the same, only the header says
  `(toml)`.
- Nix files are expressions, not values: `nix eval --json … | gestalt`.

## Install

```sh
nix run github:achimcc/gestalt -- --help
cargo install --git https://github.com/achimcc/gestalt
```

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
