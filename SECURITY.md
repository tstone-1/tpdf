# Security policy

tpdf parses attacker-controlled files for a living, and it makes two claims that are worth
holding it to: that every document is parsed and rendered in a **sandboxed worker process**
with no filesystem or network authority, and that a redaction **removes** content rather
than covering it. Reports that falsify either are the most valuable thing anyone can send.

`docs/THREAT-MODEL.md` is the worked-out position — what is being defended, the trust
boundaries, the sandbox profile in full, and the residual risks in one list. Every claim
there is either measured with the spike named, or marked untested. Read it before
reporting, and please say which claim you believe is wrong.

## Reporting a vulnerability

Use **GitHub's private vulnerability reporting** on this repository
(*Security* → *Report a vulnerability*). It is private until an advisory is published,
which is the point.

Please do not open a public issue for a suspected vulnerability.

Include a document that reproduces it if one exists. A crashing PDF is a report on its own;
you do not need to have diagnosed it.

## Scope

**In scope**

- Escaping the worker sandbox, or reaching the filesystem or network from inside it.
- Anything that causes the *application* process to parse or map a PDF engine — the
  boundary's whole purpose. Note the one documented exception: printing maps the operating
  system's own PDF parser into the app process on both platforms, which is stated rather
  than hidden, and is measured by `examples/print_probe.rs`.
- Recovering content from a document tpdf reported as successfully redacted.
- A redaction reported as **clean** that is not. A result of *not verified* is a correct
  answer, by design, and is not a vulnerability.
- Document JavaScript or launch actions executing. Both are disabled by default.
- Memory-safety defects in our own Rust reachable from document content.

**Out of scope**

- Vulnerabilities in PDFium itself. Report those to
  [Chromium](https://issues.chromium.org/); they reach far more users through Chrome than
  through tpdf. If a PDFium fix needs a pin bump here, an issue is welcome.
- The Windows build being unsigned. SmartScreen warns on first launch; this is a known and
  documented state, not a finding.
- Anything requiring an attacker who already has code execution as the user.
- Denial of service by a document that is merely large or slow. Resource bounds exist and
  are documented; a document that takes a long time is not a security issue unless it
  escapes them.

## What to expect

This is a one-person project. I will acknowledge a report within a week and tell you
whether I think it is in scope and what I intend to do. I would rather say "I am not going
to fix this and here is why" than leave a report unanswered.

If you would like credit in the advisory, say so and how you would like to be named.

## Releases

Nothing has shipped yet — there are no tags and no released binaries, so there is no
supported-version table to give. This section gets replaced with one at the first release.
