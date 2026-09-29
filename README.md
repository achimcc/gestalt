# gestalt

Shows the shape of a JSON document without showing its values.

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
| input that is not JSON | characterised (`HTML or XML`, `text`, `binary`, `truncated or broken JSON`), never echoed |

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

## Usage

```
gestalt [OPTIONS] [FILE]

  -s, --show PATH   print the values at PATH; repeatable; refused for secrets
      --hash PATH   print SHA-256 of the values at PATH; repeatable
  -n, --numbers     print the values of all numbers
  -h, --help
  -V, --version
```

Exit status: `0` described, `1` input unreadable, empty or not JSON, `2`
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

## Install

```sh
nix run github:achimcc/gestalt -- --help
cargo install --git https://github.com/achimcc/gestalt
```

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
