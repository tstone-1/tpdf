//! The timestamp client against a fake authority on 127.0.0.1.
//!
//! Every test here is offline: the authority is a `std::net::TcpListener` in a
//! thread, speaking just enough HTTP/1.1, and its tokens come from
//! `integrity/test_tsa.rs` --- minted over the imprint and nonce **the request
//! actually carried**, which the fake reads out of the bytes it was sent. So a
//! sound answer is sound only if the request was, and each fault is one token
//! property or one transport property, wrong alone.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use der::{Decode as _, Encode as _};
use sha2_10::Digest as _;

use super::*;
use crate::integrity::test_tsa::{self, mint, mint_with, Faults, Imprint, TestTsa};
use crate::integrity::Why;

/// What the signature value is, for every request here: arbitrary bytes, as
/// the imprint does not care what they are.
const VALUE: &[u8] = b"the value octets of a signature, as a SignerInfo holds them";

/// A moment inside the test authority's certificates.
const NOW: u64 = 1_790_000_000;

/// Limits short enough that a test waiting on one does not wait long.
const QUICK: Limits = Limits {
    connect: Duration::from_secs(2),
    total: Duration::from_secs(1),
    body: 64 * 1024,
};

/// What the fake authority does with a request.
enum Reply {
    /// `200 OK` with this body and its length.
    Answer(Vec<u8>),
    /// `200 OK`, the body sent chunked, with no length ahead of it.
    Chunked(Vec<u8>),
    /// This status and no body; `Location` set for a redirect.
    Status(u16, Option<String>),
    /// Reads the request, then says nothing for five seconds and hangs up.
    Silent,
}

/// A fake authority, answering one request.
struct Fake {
    url: url::Url,
    /// The request body it received, once it has.
    seen: mpsc::Receiver<Vec<u8>>,
}

/// One request's body, after the headers, by `Content-Length`.
fn read_request(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().ok()?;
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

/// Starts a fake authority whose answer `reply` decides from the request.
fn serve(reply: impl FnOnce(&[u8]) -> Reply + Send + 'static) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();
    let (tx, seen) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let Some(request) = read_request(&mut stream) else {
            return;
        };
        let _ = tx.send(request.clone());
        let head = |status: &str, extra: &str| {
            format!("HTTP/1.1 {status}\r\n{extra}Connection: close\r\n\r\n").into_bytes()
        };
        let out = match reply(&request) {
            Reply::Answer(body) => {
                let mut out = head(
                    "200 OK",
                    &format!(
                        "Content-Type: application/timestamp-reply\r\nContent-Length: {}\r\n",
                        body.len()
                    ),
                );
                out.extend(body);
                out
            }
            Reply::Chunked(body) => {
                let mut out = head(
                    "200 OK",
                    "Content-Type: application/timestamp-reply\r\nTransfer-Encoding: chunked\r\n",
                );
                for chunk in body.chunks(4096) {
                    out.extend(format!("{:x}\r\n", chunk.len()).into_bytes());
                    out.extend(chunk);
                    out.extend(b"\r\n");
                }
                out.extend(b"0\r\n\r\n");
                out
            }
            Reply::Status(code, location) => head(
                &format!("{code} Other"),
                &format!(
                    "Content-Length: 0\r\n{}",
                    location.map_or(String::new(), |l| format!("Location: {l}\r\n"))
                ),
            ),
            Reply::Silent => {
                std::thread::sleep(Duration::from_secs(5));
                return;
            }
        };
        let _ = stream.write_all(&out);
        let _ = stream.flush();
    });
    Fake {
        url: url::Url::parse(&format!("http://127.0.0.1:{port}/tsa")).expect("a URL"),
        seen,
    }
}

/// The imprint and the nonce a request carries, read with the type that wrote
/// it --- and, separately, checked below against a vector written by hand.
fn asked(request: &[u8]) -> ([u8; 32], Vec<u8>) {
    let parsed = TimeStampReq::from_der(request).expect("a TimeStampReq");
    let digest: [u8; 32] = parsed
        .message_imprint
        .hashed_message
        .as_bytes()
        .try_into()
        .expect("SHA-256");
    (digest, parsed.nonce.expect("a nonce").as_bytes().to_vec())
}

/// A `TimeStampResp` with `status`, the authority's `text`, and `token`.
fn response(status: u32, text: Option<&str>, token: Option<&[u8]>) -> Vec<u8> {
    TimeStampResp {
        status: PkiStatusInfo {
            status,
            status_string: text.map(|t| vec![t.to_string()]),
            fail_info: None,
        },
        time_stamp_token: token.map(|t| der::Any::from_der(t).expect("a token")),
    }
    .to_der()
    .expect("a response")
}

/// A fake authority granting a token minted over what was asked, with
/// `faults`, under `hash`.
fn granting(hash: Imprint, faults: Faults) -> Fake {
    serve(move |request| {
        let (digest, nonce) = asked(request);
        let imprint = if hash == Imprint::Sha256 {
            digest.to_vec()
        } else {
            hash.digest(VALUE)
        };
        let token = mint_with(hash, &imprint, Some(&nonce), NOW, &TestTsa::new(), &faults);
        Reply::Answer(test_tsa::granted(&token))
    })
}

fn ask_quick(fake: &Fake) -> Result<Vec<u8>, Refusal> {
    ask_blocking(&fake.url, VALUE, &QUICK)
}

// ------------------------------------------------------------ the control

#[test]
fn a_sound_answer_to_this_request_is_accepted() {
    // The control for every refusal below: a client that refused everything
    // would pass them all, and this is what fails it.
    let fake = granting(Imprint::Sha256, Faults::default());
    let token = ask_quick(&fake).expect("accepted");
    let request = fake.seen.recv().expect("the request arrived");
    let (digest, nonce) = asked(&request);
    let wanted: [u8; 32] = sha2_10::Sha256::digest(VALUE).into();
    assert_eq!(
        digest, wanted,
        "the imprint is SHA-256 over the value octets"
    );
    assert!(nonce.len() >= 8, "a nonce of {} bytes", nonce.len());
    let verdict = token::check(
        &token,
        token::Target::Signature(VALUE),
        &mut crate::integrity::MAX_HASHED.clone(),
    );
    assert_eq!(verdict.verdict, Verdict::Intact, "{verdict:?}");
}

#[test]
fn every_request_carries_a_nonce_of_its_own() {
    let first = granting(Imprint::Sha256, Faults::default());
    let second = granting(Imprint::Sha256, Faults::default());
    ask_quick(&first).expect("first");
    ask_quick(&second).expect("second");
    let (_, a) = asked(&first.seen.recv().expect("first request"));
    let (_, b) = asked(&second.seen.recv().expect("second request"));
    assert_ne!(a, b, "two requests carried one nonce");
}

#[test]
fn granted_with_modifications_is_granted() {
    // Status 1 carries a token as status 0 does; RFC 3161 §2.4.2.
    let fake = serve(|request| {
        let (digest, nonce) = asked(request);
        let token = mint(Imprint::Sha256, &digest, Some(&nonce), NOW, &TestTsa::new());
        Reply::Answer(response(1, None, Some(&token)))
    });
    ask_quick(&fake).expect("accepted");
}

// --------------------------------------------------- the authority's answer

#[test]
fn a_declining_status_is_refused_even_with_a_token_beside_it() {
    // A token sound in every other way, under a status that says no: the
    // status is the authority's answer, and a token beside it is not a grant.
    let fake = serve(|request| {
        let (digest, nonce) = asked(request);
        let token = mint(Imprint::Sha256, &digest, Some(&nonce), NOW, &TestTsa::new());
        Reply::Answer(response(2, Some("policy\u{7} not accepted"), Some(&token)))
    });
    match ask_quick(&fake) {
        Err(Refusal::Declined(why)) => {
            assert!(why.contains("status 2 (rejection)"), "{why}");
            assert!(why.contains("\u{201c}policy not accepted\u{201d}"), "{why}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_grant_without_a_token_is_refused() {
    let fake = serve(|_| Reply::Answer(response(0, None, None)));
    assert_eq!(ask_quick(&fake), Err(Refusal::NoToken));
}

#[test]
fn an_answer_that_is_not_a_timestamp_response_is_refused() {
    let fake = serve(|_| Reply::Answer(b"<html>a captive portal</html>".to_vec()));
    assert_eq!(ask_quick(&fake), Err(Refusal::Unreadable));
}

// ----------------------------------------------------------------- the token

#[test]
fn a_token_whose_own_signature_fails_is_refused() {
    let fake = granting(
        Imprint::Sha256,
        Faults {
            corrupt_signature: true,
            ..Faults::default()
        },
    );
    match ask_quick(&fake) {
        Err(Refusal::Token(integrity)) => assert_eq!(integrity.verdict, Verdict::Broken),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_token_of_another_signature_is_refused() {
    let fake = granting(
        Imprint::Sha256,
        Faults {
            wrong_imprint: true,
            ..Faults::default()
        },
    );
    match ask_quick(&fake) {
        Err(Refusal::Token(integrity)) => assert_eq!(integrity.verdict, Verdict::Altered),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_token_whose_own_signature_rests_on_sha1_is_refused_for_a_new_signature() {
    // Its imprint is SHA-256 and as asked; only its signature is SHA-1, so
    // only the verdict can refuse it. A reader would see `weak` beside a
    // signature made today, and tpdf does not write one.
    let fake = granting(
        Imprint::Sha256,
        Faults {
            sha1_signature: true,
            ..Faults::default()
        },
    );
    match ask_quick(&fake) {
        Err(Refusal::Token(integrity)) => assert_eq!(integrity.verdict, Verdict::Weak),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_token_that_binds_no_certificate_is_refused() {
    let fake = granting(
        Imprint::Sha256,
        Faults {
            binding: test_tsa::Binding::Neither,
            ..Faults::default()
        },
    );
    match ask_quick(&fake) {
        Err(Refusal::Token(integrity)) => {
            assert_eq!(integrity.verdict, Verdict::Unchecked);
            assert_eq!(integrity.why, Some(Why::Binding));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_token_under_another_hash_than_the_one_asked_for_is_refused() {
    // SHA-384 over the same value: the verdict is intact, because a token may
    // use any hash --- so only the comparison with the request can refuse it.
    let fake = granting(Imprint::Sha384, Faults::default());
    assert_eq!(ask_quick(&fake), Err(Refusal::Imprint));
}

#[test]
fn a_token_carrying_another_nonce_or_none_is_refused() {
    // Sound tokens over the right imprint: what is wrong is only that they do
    // not answer *this* request, which is what a replayed answer looks like.
    let other = serve(|request| {
        let (digest, mut nonce) = asked(request);
        nonce[0] ^= 0x01;
        let token = mint(Imprint::Sha256, &digest, Some(&nonce), NOW, &TestTsa::new());
        Reply::Answer(test_tsa::granted(&token))
    });
    assert_eq!(ask_quick(&other), Err(Refusal::Nonce));
    let none = serve(|request| {
        let (digest, _) = asked(request);
        let token = mint(Imprint::Sha256, &digest, None, NOW, &TestTsa::new());
        Reply::Answer(test_tsa::granted(&token))
    });
    assert_eq!(ask_quick(&none), Err(Refusal::Nonce));
}

#[test]
fn a_nonce_is_compared_as_a_number() {
    // A nonce with leading zero bytes is the same INTEGER without them.
    let token = mint(
        Imprint::Sha256,
        &sha2_10::Sha256::digest(VALUE),
        Some(&[1, 2, 3]),
        NOW,
        &TestTsa::new(),
    );
    let answer = test_tsa::granted(&token);
    accept(&answer, VALUE, &[0, 0, 1, 2, 3]).expect("the same number");
    assert_eq!(accept(&answer, VALUE, &[1, 2, 4]), Err(Refusal::Nonce));
}

// ------------------------------------------------------------- the transport

#[test]
fn an_answer_longer_than_the_bound_is_refused_whether_or_not_it_says_so() {
    // Once with its length ahead of it, once chunked with none: the bound is
    // on what is read, not on what the server announces.
    let long = vec![0x30u8; QUICK.body + 1];
    let said = serve({
        let long = long.clone();
        move |_| Reply::Answer(long)
    });
    assert_eq!(ask_quick(&said), Err(Refusal::TooLarge));
    let unsaid = serve(move |_| Reply::Chunked(long));
    assert_eq!(ask_quick(&unsaid), Err(Refusal::TooLarge));
}

#[test]
fn a_server_that_never_answers_is_given_up_on_within_the_limit() {
    let fake = serve(|_| Reply::Silent);
    // The first client a process builds reads the system's proxy settings,
    // and in a test binary that took 3.8 to 20 s --- CoreFoundation listing
    // the directory the executable sits in, which for a test is
    // `target/debug/deps` (`docs/TRAPS.md`). Paid here, before the clock
    // starts, because it is not the request's time and no limit covers it.
    client(&QUICK).expect("a client");
    let started = Instant::now();
    let result = ask_quick(&fake);
    let took = started.elapsed();
    assert_eq!(result, Err(Refusal::TimedOut));
    // The fake hangs up after five seconds; a client with no total limit
    // would have waited for that and reported a closed connection instead.
    assert!(took < Duration::from_secs(4), "{took:?}");
}

#[test]
fn a_refused_connection_is_unreachable() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let port = listener.local_addr().expect("an address").port();
    drop(listener);
    let url = url::Url::parse(&format!("http://127.0.0.1:{port}/")).expect("a URL");
    // Not `QUICK`: Windows answers a connection to a closed local port by
    // retrying the SYN for about two seconds before it reports the refusal, so
    // QUICK's one-second total expired first and this read `TimedOut` on the
    // first Windows CI run (2026-09-28). The shipped limits are ten seconds to
    // connect and thirty in total, which is why the product was right and only
    // this test was not. The limits here leave that delay room and still bound
    // the test.
    let refused = Limits {
        connect: Duration::from_secs(10),
        total: Duration::from_secs(10),
        ..QUICK
    };
    match ask_blocking(&url, VALUE, &refused) {
        Err(Refusal::Unreachable(_)) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_http_error_or_a_redirect_is_refused_and_the_redirect_is_not_followed() {
    let failing = serve(|_| Reply::Status(500, None));
    assert_eq!(ask_quick(&failing), Err(Refusal::Http(500)));

    // Where the redirect points: a server that would grant, and must not be
    // asked anything.
    let elsewhere = granting(Imprint::Sha256, Faults::default());
    let to = elsewhere.url.to_string();
    let moving = serve(move |_| Reply::Status(302, Some(to)));
    assert_eq!(ask_quick(&moving), Err(Refusal::Http(302)));
    assert!(
        elsewhere
            .seen
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "the redirect was followed"
    );
}

// ------------------------------------------------------------ the addresses

#[test]
fn only_http_and_https_addresses_with_a_host_and_no_credentials_are_asked() {
    for good in [
        "http://timestamp.digicert.com",
        "https://timestamp.sectigo.com",
        "http://127.0.0.1:8080/tsa",
        "  https://tsa.example/rfc3161  ",
    ] {
        let url = authority(good).unwrap_or_else(|e| panic!("{good}: {e:?}"));
        assert!(matches!(url.scheme(), "http" | "https"), "{good}");
    }
    for bad in [
        "ftp://timestamp.example/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:application/timestamp-reply,00",
        "timestamp.digicert.com",
        "http://user:secret@tsa.example/",
        "http://user@tsa.example/",
        "",
    ] {
        match authority(bad) {
            Err(Refusal::Address(_)) => {}
            other => panic!("{bad:?}: {other:?}"),
        }
    }
}

#[test]
fn a_non_web_scheme_is_refused_before_any_socket_is_opened() {
    // A listener at the address an `ftp:` URL names: whatever the refusal
    // says, nothing may connect to it.
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    listener.set_nonblocking(true).expect("non-blocking");
    let port = listener.local_addr().expect("an address").port();
    for scheme in ["ftp", "gopher", "ws"] {
        assert!(authority(&format!("{scheme}://127.0.0.1:{port}/")).is_err());
    }
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "something connected"
    );
}

#[test]
fn the_short_list_is_reached_by_name() {
    for server in SERVERS {
        let url = authority(server.name).expect("a name on the list");
        assert_eq!(url.as_str().trim_end_matches('/'), server.url);
        assert_eq!(
            authority(&server.name.to_uppercase()).expect("any case"),
            url
        );
    }
    // Every server on the list is one `authority` accepts as a URL too.
    for server in SERVERS {
        authority(server.url).expect("a URL on the list");
    }
    assert!(authority("verisign").is_err());
}

// --------------------------------------------------------------- the request

#[test]
fn the_request_is_the_der_rfc_3161_describes() {
    // Written out by hand, field by field, rather than read back with the type
    // that wrote it --- a writer and its own reader agree about output that is
    // wrong. `openssl ts -query -in <these bytes> -text` prints version 1,
    // sha256, the 32 bytes, the nonce 0x80..0f and `Certificate required: yes`
    // (docs/PLAN.md records the run).
    let nonce: [u8; 16] = [
        0x80, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];
    let der = request(&[0x11; 32], &nonce).expect("encoded");
    let mut want = vec![0x30, 0x4a];
    want.extend([0x02, 0x01, 0x01]); // version 1
    want.extend([0x30, 0x2f]); // MessageImprint
    want.extend([0x30, 0x0b, 0x06, 0x09]); // AlgorithmIdentifier, no parameters
    want.extend([0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01]); // id-sha256
    want.extend([0x04, 0x20]);
    want.extend([0x11; 32]);
    want.extend([0x02, 0x11, 0x00]); // the nonce, a zero ahead of its top bit
    want.extend(nonce);
    want.extend([0x01, 0x01, 0xff]); // certReq TRUE
    assert_eq!(der, want);
}

// ------------------------------------------------------------ never silently

#[test]
fn a_timestamp_asked_for_and_not_had_is_an_error_never_a_signature_without_one() {
    use crate::sign_cms::testkeys::{self, certificate, Soft, Spec, NOW as SIGNED_AT};

    let original = testkeys::plain_pdf();
    let key = Soft::p256(3);
    let cert = certificate(&key, &Spec::new("Signer"));
    let made = || {
        let unsigned =
            crate::sign_prepare::prepare(original.clone(), SIGNED_AT, None).expect("prepared");
        crate::sign_cms::sign(original.clone(), unsigned, SIGNED_AT, &cert, &[], &key)
            .expect("made")
    };
    let url = authority("http://127.0.0.1:9/").expect("a URL");

    // Not asked: nothing is requested, and there is nothing to add.
    let mut asked = false;
    let none = stamp(&made(), None, |_, _| {
        asked = true;
        Err(Refusal::TimedOut)
    });
    assert_eq!(none, Ok(None));
    assert!(!asked, "a request was made that nobody asked for");

    // Asked, and it failed: the failure, never `None`.
    for failure in [
        Refusal::TimedOut,
        Refusal::Unreachable("down".into()),
        Refusal::Nonce,
    ] {
        let result = stamp(&made(), Some(&url), |_, _| Err(failure.clone()));
        assert_eq!(result, Err(failure));
    }

    // Asked, and answered: a blob carrying the token, which seals intact.
    let signature = made();
    let value = signature.value().expect("a value");
    let token = mint(
        Imprint::Sha256,
        &sha2_10::Sha256::digest(&value),
        Some(&[9; 8]),
        SIGNED_AT,
        &TestTsa::new(),
    );
    let stamped = stamp(&signature, Some(&url), |_, asked_for| {
        assert_eq!(asked_for, value.as_slice(), "the request is over the value");
        Ok(token.clone())
    })
    .expect("stamped")
    .expect("a blob");
    signature.seal(Some(stamped)).expect("sealed");
}

#[test]
fn every_refusal_names_the_authority() {
    let integrity = crate::integrity::Integrity {
        verdict: Verdict::Broken,
        ..Default::default()
    };
    for refusal in [
        Refusal::Unreachable("connection refused".into()),
        Refusal::TimedOut,
        Refusal::Http(503),
        Refusal::TooLarge,
        Refusal::Unreadable,
        Refusal::Declined("status 2 (rejection)".into()),
        Refusal::NoToken,
        Refusal::Token(integrity.clone()),
        Refusal::Imprint,
        Refusal::Nonce,
        Refusal::Fit("too long".into()),
    ] {
        let sentence = refusal.sentence("timestamp.example");
        assert!(sentence.contains("timestamp.example"), "{sentence}");
    }
}

// ---------------------------------------------- the archive timestamp's range

#[test]
fn an_archive_timestamp_is_asked_over_the_covered_range_and_checked_over_it() {
    // PAdES B-LTA: the imprint is SHA-256 over the pieces the range covers,
    // in order, and the token is checked over those pieces --- not over a
    // signature's value.
    let pieces: [&[u8]; 2] = [b"%PDF-1.7 before the hole", b" after it %%EOF"];
    let fake = granting(Imprint::Sha256, Faults::default());
    let token = ask_over_range_blocking(&fake.url, &pieces, &QUICK).expect("accepted");
    let request = fake.seen.recv().expect("the request arrived");
    let (digest, _) = asked(&request);
    let wanted: [u8; 32] = sha2_10::Sha256::digest(pieces.concat()).into();
    assert_eq!(digest, wanted, "the imprint is over the pieces, joined");
    let verdict = token::check(
        &token,
        token::Target::Range(&pieces),
        &mut crate::integrity::MAX_HASHED.clone(),
    );
    assert_eq!(verdict.verdict, Verdict::Intact, "{verdict:?}");

    // A token over anything else is refused, as a signature's is.
    let fake = granting(
        Imprint::Sha256,
        Faults {
            wrong_imprint: true,
            ..Faults::default()
        },
    );
    assert!(matches!(
        ask_over_range_blocking(&fake.url, &pieces, &QUICK),
        Err(Refusal::Token(_) | Refusal::Imprint)
    ));
}
