/** Signing, certificates and long-term validation data: `sign_prepare.rs`
 * (and its `appearance.rs`), `sign_cms.rs`, `sign_dss.rs`, `pkcs11.rs`,
 * `pkcs12.rs`, `pin_prompt.rs`, `longterm.rs` (and `existing.rs`), `tsa.rs`,
 * `keystore.rs`, `certfiles.rs`, `signature_store.rs`, `docinfo.rs`,
 * `words.rs`, `commands/sign.rs` and `commands/validation.rs`.
 *
 * Several sentences take a phrase another sentence made, and German declines
 * it where English does not; the helpers below put a nominative phrase into
 * the case the sentence around it needs. A phrase they do not know is left as
 * it is.
 */

import type { Wording } from "./de";

/** A phrase starting with the article `from`, with `to` in its place. */
function swap(phrase: string, from: string, to: string): string {
  return phrase.startsWith(`${from} `) ? `${to}${phrase.slice(from.length)}` : phrase;
}

/** "der Zeitstempeldienst ..." as an accusative object. */
const accusative = (phrase: string): string => swap(phrase, "der", "den");

/** "der Zeitstempeldienst ..." after "von". */
const fromThe = (phrase: string): string =>
  phrase.startsWith("der ") ? `vom${phrase.slice(3)}` : `von ${phrase}`;

/** A feminine phrase ("die OCSP-Antwort ...") after "laut". */
const dative = (phrase: string): string => swap(phrase, "die", "der");

/** `whose` as Rust passes it, as a genitive. */
const OWNER: Record<string, string> = {
  "the signer's": "des Unterzeichners",
  "the authority's": "des Zeitstempeldienstes",
  "the timestamp authority's": "des Zeitstempeldienstes",
};
const owner = (whose: string): string => OWNER[whose] ?? whose;

/** `words::computer`, nominative and dative. */
const COMPUTER: Record<string, [string, string]> = {
  "this Mac": ["dieser Mac", "diesem Mac"],
  "this PC": ["dieser PC", "diesem PC"],
  "this computer": ["dieser Computer", "diesem Computer"],
};
const computer = (name: string, which: 0 | 1): string => COMPUTER[name]?.[which] ?? name;

/** The notes a signature carries (`sign_prepare.rs`, `appearance.rs`): all masculine. */
const NOTE: Record<string, string> = { contact: "Kontakt", reason: "Grund", location: "Ort" };
const note = (what: string): string => NOTE[what] ?? what;

/** `words::verdict_briefly`, which the window passes on in English. */
const READS: Record<string, string> = {
  intact: "intakt",
  intakt: "intakt",
  altered: "verändert",
  broken: "fehlerhaft",
  "not checked": "nicht geprüft",
  "unchanged under SHA-1 only": "nur unter SHA-1 unverändert",
};

/** `sign_cms::purposes_named`, joined with " and ". */
const PURPOSE: Record<string, string> = {
  "web servers": "Webserver",
  "logging in": "Anmelden",
  "code signing": "Codesignatur",
  timestamping: "Zeitstempel",
  "revocation responses": "Widerrufsauskünfte",
  "other purposes": "andere Zwecke",
};

/** `words::after_long_term`'s list of fields, after "für". */
function listed(text: string): string {
  return text
    .replace(/^the signatures /, "die Signaturen ")
    .replace(/^the signature /, "die Signatur ")
    .replace(/^the timestamps /, "die Zeitstempel ")
    .replace(/^the timestamp /, "den Zeitstempel ")
    .replace(/ and the timestamps /, " und die Zeitstempel ")
    .replace(/ and the timestamp /, " und den Zeitstempel ");
}

const EACH: Record<string, string> = {
  "that signature is": "diese Signatur ist",
  "that timestamp is": "dieser Zeitstempel ist",
  "each of them is": "alle sind",
};

export default {
  // sign_dss.rs
  "this document is encrypted, and tpdf adds long-term validation data only to a document that is not":
    "dieses Dokument ist verschlüsselt, und tpdf fügt Langzeitvalidierungsdaten nur einem unverschlüsselten Dokument hinzu",
  "this document could not be parsed: {e}": "dieses Dokument konnte nicht gelesen werden: {e}",
  "this document could not be read: {why}": "dieses Dokument konnte nicht ausgelesen werden: {why}",
  "this document's signatures and the certificates it already carries are {bytes} bytes together, more than the {bound} tpdf reads to add validation data":
    "die Signaturen dieses Dokuments und die Zertifikate, die es schon enthält, sind zusammen {bytes} Bytes groß, mehr als die {bound}, die tpdf zum Hinzufügen von Validierungsdaten liest",
  "this document's /DSS is not a dictionary": "das /DSS dieses Dokuments ist kein Dictionary",
  "this document's /DSS /{key} is not an array": "/DSS /{key} in diesem Dokument ist kein Array",
  "there is no revocation data to add": "es gibt keine Widerrufsdaten zum Hinzufügen",
  "the validation data is {bytes} bytes, more than the {MAX_BYTES} tpdf adds":
    "die Validierungsdaten sind {bytes} Bytes groß, mehr als die {MAX_BYTES}, die tpdf hinzufügt",
  "the signed document could not be parsed: {e}": "das signierte Dokument konnte nicht gelesen werden: {e}",
  "an encrypted document is not signed by tpdf, and gets no /DSS":
    "ein verschlüsseltes Dokument wird nicht von tpdf signiert und bekommt kein /DSS",
  "the document's catalog is not an object of its own": "der Katalog des Dokuments ist kein eigenes Objekt",
  "the document's catalog is not a dictionary": "der Katalog des Dokuments ist kein Dictionary",
  "could not build the validation data's revision: {e}":
    "die Revision mit den Validierungsdaten konnte nicht erstellt werden: {e}",

  // pkcs12.rs
  "The password is not the one this certificate file was saved with.":
    "Das Passwort ist nicht das, mit dem diese Zertifikatsdatei gespeichert wurde.",
  "This certificate file holds an RSA key, and tpdf signs from a file only with an ECDSA key. A smart card or token with this certificate can sign.":
    "Diese Zertifikatsdatei enthält einen RSA-Schlüssel, und tpdf signiert aus einer Datei nur mit einem ECDSA-Schlüssel. Eine Smartcard oder ein Token mit diesem Zertifikat kann signieren.",
  "This certificate file has no private key for its certificate, so it cannot sign.":
    "Diese Zertifikatsdatei hat keinen privaten Schlüssel zu ihrem Zertifikat und kann deshalb nicht signieren.",
  "This certificate file uses {what}, which tpdf does not read.":
    "Diese Zertifikatsdatei verwendet etwas, das tpdf nicht liest: {what}.",
  "This is not a certificate file tpdf can read: {why}.": "Das ist keine Zertifikatsdatei, die tpdf lesen kann: {why}.",
  "the key did not sign: {e}": "der Schlüssel hat nicht signiert: {e}",
  "encrypted contents with no content": "verschlüsselte Inhalte ohne Inhalt",
  "contents of type {other}": "Inhalte vom Typ {other}",
  "outer contents of type {kind}": "äußere Inhalte vom Typ {kind}",
  "a password with characters outside the Basic Multilingual Plane":
    "ein Passwort mit Zeichen außerhalb der Basic Multilingual Plane",
  "a MAC key of no length": "ein MAC-Schlüssel ohne Länge",
  "a PBMAC1 integrity check": "eine PBMAC1-Integritätsprüfung",
  "an integrity check with the hash {other}": "eine Integritätsprüfung mit dem Hash {other}",
  "the encryption scheme it names ({e})": "das Verschlüsselungsverfahren, das sie nennt ({e})",
  "an encryption scheme with no parameters": "ein Verschlüsselungsverfahren ohne Parameter",
  "a triple-DES key of the wrong length": "ein Triple-DES-Schlüssel mit falscher Länge",
  "an RC2 IV of the wrong length": "ein RC2-IV mit falscher Länge",
  "RC4 encryption": "RC4-Verschlüsselung",
  "the encryption scheme {other}": "das Verschlüsselungsverfahren {other}",
  "a key of type {other}": "ein Schlüssel vom Typ {other}",
  "an EC private key that is not a valid scalar": "ein privater EC-Schlüssel, der kein gültiger Skalar ist",
  "the curve {other}": "die Kurve {other}",
  "an EC private key that names no curve": "ein privater EC-Schlüssel, der keine Kurve nennt",

  // pkcs11.rs, pin_prompt.rs
  "the PIN for the token {token:?} is not correct, and nothing was signed. A token locks itself after a few wrong PINs.":
    "die PIN für das Token {token} ist nicht richtig, und es wurde nichts signiert. Ein Token sperrt sich nach einigen falschen PINs.",
  "the token {token:?} is locked after too many wrong PINs, and nothing was signed. Its unblocking code (PUK) or its issuer can unlock it.":
    "das Token {token} ist nach zu vielen falschen PINs gesperrt, und es wurde nichts signiert. Sein Entsperrcode (PUK) oder sein Herausgeber kann es entsperren.",
  "the PIN has the wrong number of characters for the token {token:?}, and nothing was signed":
    "die PIN hat für das Token {token} die falsche Anzahl Zeichen, und es wurde nichts signiert",
  "the token {token:?} did not accept the PIN: {other}": "das Token {token} hat die PIN nicht angenommen: {other}",
  "the token {token:?} needs its PIN, and nothing here can ask for it":
    "das Token {token} braucht seine PIN, und hier kann nichts danach fragen",
  "the certificate file {file:?} needs its password, and nothing here can ask for it":
    "die Zertifikatsdatei {file} braucht ihr Passwort, und hier kann nichts danach fragen",
  "signing with a smart card or token needs p11-kit (the package p11-kit-modules on Ubuntu and Debian), and it could not be loaded: {tried}":
    "Signieren mit einer Smartcard oder einem Token braucht p11-kit (das Paket p11-kit-modules unter Ubuntu und Debian), und es konnte nicht geladen werden: {tried}",
  "listing smart cards and tokens: {e}": "beim Auflisten der Smartcards und Tokens: {e}",
  "opening the token {token:?}: {e}": "beim Öffnen des Tokens {token}: {e}",
  "reading the token {token:?}: {e}": "beim Lesen des Tokens {token}: {e}",
  "signing was cancelled": "das Signieren wurde abgebrochen",
  "finding the key on {token:?}: {e}": "beim Suchen des Schlüssels auf {token}: {e}",
  "the token {token:?} has this certificate but no key for it":
    "das Token {token} hat dieses Zertifikat, aber keinen Schlüssel dazu",
  "the token did not sign: {e}": "das Token hat nicht signiert: {e}",
  "the PIN dialog could not be shown: {e}": "der PIN-Dialog konnte nicht angezeigt werden: {e}",
  "the PIN dialog closed without an answer": "der PIN-Dialog wurde ohne Antwort geschlossen",

  // tsa.rs
  "the timestamp authority": "der Zeitstempeldienst",
  "the timestamp authority at {host}": "der Zeitstempeldienst unter {host}",
  "tpdf does not ask {why}": "tpdf fragt hier nicht an: {why}",
  "tpdf could not reach {at}: {why}": (a) => `tpdf konnte ${accusative(a.at!)} nicht erreichen: ${a.why}`,
  "{at} did not answer within {secs} seconds": "{at} hat nicht innerhalb von {secs} Sekunden geantwortet",
  "{at} answered with HTTP status {code}": "{at} hat mit HTTP-Status {code} geantwortet",
  "{at} answered with more than {kib} KiB, which no timestamp needs":
    "{at} hat mit mehr als {kib} KiB geantwortet, die kein Zeitstempel braucht",
  "{at} answered with something that is not a timestamp": "{at} hat mit etwas geantwortet, das kein Zeitstempel ist",
  "{at} declined: {why}": "{at} hat abgelehnt: {why}",
  "{at} said it granted a timestamp and sent none": "{at} hat einen Zeitstempel zugesagt und keinen geschickt",
  "{at} sent a timestamp that does not check out ({why})":
    "{at} hat einen Zeitstempel geschickt, der der Prüfung nicht standhält ({why})",
  "{at} sent a timestamp that is not of this signature as it was asked for (SHA-256)":
    "{at} hat einen Zeitstempel geschickt, der nicht über diese Signatur geht, wie sie angefragt wurde (SHA-256)",
  "{at} sent a timestamp that does not answer this request (its nonce is not the one sent)":
    "{at} hat einen Zeitstempel geschickt, der nicht auf diese Anfrage antwortet (seine Nonce ist nicht die gesendete)",
  "the timestamp from {at} could not be added: {why}": (a) =>
    `der Zeitstempel ${fromThe(a.at!)} konnte nicht hinzugefügt werden: ${a.why}`,
  intact: "intakt",
  "it rests on {digest}": "er beruht auf {digest}",
  "it is of something other than this signature": "er gilt für etwas anderes als diese Signatur",
  "its own signature fails": "seine eigene Signatur schlägt fehl",
  "it could not be checked: {why}": "er konnte nicht geprüft werden: {why}",
  "no reason": "kein Grund",
  "`{text}` for a timestamp: it is not a URL, nor one of {servers}":
    "„{text}“ als Zeitstempeldienst: das ist keine URL und keiner von {servers}",
  "a timestamp authority over `{scheme}:` --- only http and https":
    "ein Zeitstempeldienst über „{scheme}:“ - nur http und https sind möglich",
  "`{text}` for a timestamp: it names no server": "„{text}“ als Zeitstempeldienst: das nennt keinen Server",
  "a timestamp authority with a user name or password in its address":
    "ein Zeitstempeldienst mit Benutzername oder Passwort in seiner Adresse",
  "the system's random source failed": "die Zufallsquelle des Systems hat versagt",

  // keystore.rs, certfiles.rs, signature_store.rs
  "that certificate is no longer in your keychain or certificate store":
    "dieses Zertifikat ist nicht mehr in deinem Schlüsselbund oder Zertifikatspeicher",
  "the keychain could not be searched: {error}": "der Schlüsselbund konnte nicht durchsucht werden: {error}",
  "the keychain would not give access to this key: {e}":
    "der Schlüsselbund hat keinen Zugriff auf diesen Schlüssel gegeben: {e}",
  "macOS did not sign: {e}": "macOS hat nicht signiert: {e}",
  "the certificate store could not be opened: {e}": "der Zertifikatspeicher konnte nicht geöffnet werden: {e}",
  "Windows would not give access to this certificate's key (only keys held through CNG can sign): {e}":
    "Windows hat keinen Zugriff auf den Schlüssel dieses Zertifikats gegeben (nur über CNG gehaltene Schlüssel können signieren): {e}",
  "this certificate's key is held by a provider tpdf does not use":
    "der Schlüssel dieses Zertifikats liegt bei einem Anbieter, den tpdf nicht verwendet",
  "Windows did not sign (0x{status:08X})": "Windows hat nicht signiert (0x{status})",
  "the certificate file holds a {held} key, and the signature asked for {asked}":
    "die Zertifikatsdatei enthält einen {held}-Schlüssel, und die Signatur hat {asked} verlangt",
  "The list of certificate files {path} could not be read: {e}.":
    "Die Liste der Zertifikatsdateien {path} konnte nicht gelesen werden: {e}.",
  "The list of certificate files {path} is damaged ({e}).": "Die Liste der Zertifikatsdateien {path} ist beschädigt ({e}).",
  "{why} Nothing was added; move it aside to start a new list.":
    "{why} Es wurde nichts hinzugefügt; verschiebe sie, um eine neue Liste zu beginnen.",
  "The certificate file could not be read: {e}.": "Die Zertifikatsdatei konnte nicht gelesen werden: {e}.",
  "The list of certificate files could not be saved: {e}.":
    "Die Liste der Zertifikatsdateien konnte nicht gespeichert werden: {e}.",
  "The certificate file {path} could not be read: {e}.": "Die Zertifikatsdatei {path} konnte nicht gelesen werden: {e}.",
  "The certificate file {path} now holds a different certificate from the one added; add it again.":
    "Die Zertifikatsdatei {path} enthält jetzt ein anderes Zertifikat als das hinzugefügte; füge sie erneut hinzu.",
  "invalid signature pixels": "ungültige Pixel der Unterschrift",
  "invalid saved signature size": "ungültige Größe der gespeicherten Unterschrift",
  "invalid saved signature pixels": "ungültige Pixel der gespeicherten Unterschrift",
  "Keychain could not read the saved signature: {error}":
    "Der Schlüsselbund konnte die gespeicherte Unterschrift nicht lesen: {error}",
  "Keychain could not save the signature: {e}": "Der Schlüsselbund konnte die Unterschrift nicht speichern: {e}",
  "Keychain could not remove the signature: {error}": "Der Schlüsselbund konnte die Unterschrift nicht entfernen: {error}",
  "Windows could not protect or unlock the signature: {e}":
    "Windows konnte die Unterschrift nicht schützen oder entsperren: {e}",
  "invalid protected signature size": "ungültige Größe der geschützten Unterschrift",
  "saved signature exceeds its size limit": "die gespeicherte Unterschrift überschreitet ihre Größengrenze",
  "missing signature directory": "der Ordner für die Unterschrift fehlt",
  "tpdf does not keep a signature on Linux yet; draw it or import it each time":
    "tpdf speichert unter Linux noch keine Unterschrift; zeichne oder importiere sie jedes Mal",

  // docinfo.rs
  "this form has more fields, or groups nested deeper, than tpdf reads, so it cannot tell whether a signature locked the document":
    "dieses Formular hat mehr Felder oder tiefer verschachtelte Gruppen, als tpdf liest, daher kann es nicht sagen, ob eine Signatur das Dokument gesperrt hat",

  // sign_prepare.rs
  "the {what} is longer than the {MAX_NOTE_CHARS} characters a signature carries": (a) =>
    `der ${note(a.what!)} ist länger als die ${a.MAX_NOTE_CHARS} Zeichen, die eine Signatur trägt`,
  "the {what} has a control character in it, which a signature does not carry": (a) =>
    `der ${note(a.what!)} enthält ein Steuerzeichen, das eine Signatur nicht trägt`,
  "this document's form could not be read: {why}": "das Formular dieses Dokuments konnte nicht gelesen werden: {why}",
  "this document has no field called `{name}`": "dieses Dokument hat kein Feld namens „{name}“",
  "`{name}` is shown in {count} places, and a signature goes in one":
    "„{name}“ wird an {count} Stellen angezeigt, und eine Signatur kommt an eine",
  "`{name}` already holds a signature": "„{name}“ enthält schon eine Signatur",
  "`{name}` is not a signature field": "„{name}“ ist kein Signaturfeld",
  "`{name}` is read-only, so it cannot be signed": "„{name}“ ist schreibgeschützt und kann deshalb nicht signiert werden",
  "the page the signature field is on is not in this document":
    "die Seite, auf der das Signaturfeld liegt, ist nicht in diesem Dokument",
  "`{name}` asks its signer to lock other fields of the form, which tpdf does not do, so it does not sign this field":
    "„{name}“ verlangt von seinem Unterzeichner, andere Felder des Formulars zu sperren, was tpdf nicht tut, daher signiert es dieses Feld nicht",
  "`{name}` sets requirements its signature has to meet, which tpdf does not check, so it does not sign this field":
    "„{name}“ stellt Anforderungen an seine Signatur, die tpdf nicht prüft, daher signiert es dieses Feld nicht",
  "a signature preview is between {least} and {PREVIEW_MAX_SIDE} points a side":
    "eine Signaturvorschau hat eine Seitenlänge zwischen {least} und {PREVIEW_MAX_SIDE} Punkt",
  "could not build the preview's page: {e}": "die Seite der Vorschau konnte nicht erstellt werden: {e}",
  "tpdf cannot sign an encrypted document yet: the signature's value would be encrypted with the document, and no reader could check it.":
    "tpdf kann ein verschlüsseltes Dokument noch nicht signieren: Der Wert der Signatur würde mit dem Dokument verschlüsselt, und kein Programm könnte ihn prüfen.",
  "This document is certified with no changes permitted, so a further signature would break its certification.":
    "Dieses Dokument ist ohne erlaubte Änderungen zertifiziert, daher würde eine weitere Signatur seine Zertifizierung brechen.",
  "A signature locked this document with no changes permitted, so a further signature would break that lock.":
    "Eine Signatur hat dieses Dokument ohne erlaubte Änderungen gesperrt, daher würde eine weitere Signatur diese Sperre brechen.",
  "This document is certified, and its certification already says which changes are permitted. tpdf does not add a second statement beside it: sign it without locking.":
    "Dieses Dokument ist zertifiziert, und seine Zertifizierung legt schon fest, welche Änderungen erlaubt sind. tpdf fügt daneben keine zweite Festlegung hinzu: Signiere es ohne Sperren.",
  "This document is certified, and tpdf draws no visible signature on a certified document: one added as a new field is read by at least one widely used validator as a change the certification does not permit, and whether one drawn in a field the document already has is read the same way has not been measured. Sign the field without a visible appearance.":
    "Dieses Dokument ist zertifiziert, und tpdf zeichnet keine sichtbare Signatur auf ein zertifiziertes Dokument: Eine als neues Feld hinzugefügte liest mindestens ein verbreitetes Prüfprogramm als Änderung, die die Zertifizierung nicht erlaubt, und ob eine in ein vorhandenes Feld gezeichnete genauso gelesen wird, ist nicht gemessen. Signiere das Feld ohne sichtbares Erscheinungsbild.",
  "This document is certified, and a visible signature added after a certification is read by at least one widely used validator as a change the certification does not permit. Sign it without a visible appearance --- an invisible signature is accepted.":
    "Dieses Dokument ist zertifiziert, und eine nach einer Zertifizierung hinzugefügte sichtbare Signatur liest mindestens ein verbreitetes Prüfprogramm als Änderung, die die Zertifizierung nicht erlaubt. Signiere es ohne sichtbares Erscheinungsbild - eine unsichtbare Signatur wird akzeptiert.",
  "this document has no page to put a signature on": "dieses Dokument hat keine Seite, auf die eine Signatur kommen könnte",
  "the field is {width} by {height} points, smaller than the {least} a side a visible signature is drawn in --- sign it without an appearance":
    "das Feld ist {width} mal {height} Punkt groß, kleiner als die {least} Punkt Seitenlänge, in denen eine sichtbare Signatur gezeichnet wird - signiere es ohne Erscheinungsbild",
  "the page chosen for the signature is not in this document":
    "die für die Signatur gewählte Seite ist nicht in diesem Dokument",
  "this document's catalog is not an object of its own": "der Katalog dieses Dokuments ist kein eigenes Objekt",
  "could not bring the signature field across: {e}": "das Signaturfeld konnte nicht übernommen werden: {e}",
  "could not build the signature's revision: {e}": "die Revision der Signatur konnte nicht erstellt werden: {e}",
  "{why}, and a new signature field needs a name none has":
    "{why}, und ein neues Signaturfeld braucht einen Namen, den noch keines hat",
  "the page the signature goes on is not a dictionary": "die Seite, auf die die Signatur kommt, ist kein Dictionary",
  "the annotation list of the page the signature goes on": "die Annotationsliste der Seite, auf die die Signatur kommt",
  "this document's catalog is not a dictionary": "der Katalog dieses Dokuments ist kein Dictionary",
  "this document's form is not a dictionary": "das Formular dieses Dokuments ist kein Dictionary",
  "this document's list of form fields": "die Liste der Formularfelder dieses Dokuments",
  "{what} is not an array": "{what} ist kein Array",
  "could not bring the signature's page across: {e}": "die Seite der Signatur konnte nicht übernommen werden: {e}",
  "could not bring the field list across: {e}": "die Feldliste konnte nicht übernommen werden: {e}",
  "could not bring the form across: {e}": "das Formular konnte nicht übernommen werden: {e}",
  "tpdf could not find the signature's {what} exactly once in the revision it built, so it will not guess where to sign":
    (a) =>
      `tpdf hat ${a.what === "byte range" ? "den Bytebereich" : a.what === "value" ? "den Wert" : a.what} der ` +
      "Signatur nicht genau einmal in der erstellten Revision gefunden und rät deshalb nicht, wo signiert wird",
  "this document is too large for tpdf to sign": "dieses Dokument ist zu groß, als dass tpdf es signieren könnte",

  // sign_prepare/appearance.rs
  "the text has a `}}` that closes nothing in `{line}` --- write `}}}}` for a brace":
    "der Text hat in „{line}“ eine „}“, die nichts schließt - schreibe „}}“ für eine Klammer",
  "the text has a `{{` that is never closed in `{line}` --- write `{{{{` for a brace":
    "der Text hat in „{line}“ eine „{“, die nie geschlossen wird - schreibe „{{“ für eine Klammer",
  "the {what} is longer than the {MAX_NOTE_CHARS} characters a visible signature draws": (a) =>
    `der ${note(a.what!)} ist länger als die ${a.MAX_NOTE_CHARS} Zeichen, die eine sichtbare Signatur zeichnet`,
  "the reason, {text}, has characters tpdf cannot draw in a visible signature yet (it draws Latin-1 only), and drawing others would put different words on the page --- change the reason, or leave it empty":
    "der Grund, {text}, hat Zeichen, die tpdf in einer sichtbaren Signatur noch nicht zeichnen kann (es zeichnet nur Latin-1), und andere zu zeichnen würde andere Wörter auf die Seite setzen - ändere den Grund oder lass ihn leer",
  "the location, {text}, has characters tpdf cannot draw in a visible signature yet (it draws Latin-1 only), and drawing others would put different words on the page --- change the location, or leave it empty":
    "der Ort, {text}, hat Zeichen, die tpdf in einer sichtbaren Signatur noch nicht zeichnen kann (es zeichnet nur Latin-1), und andere zu zeichnen würde andere Wörter auf die Seite setzen - ändere den Ort oder lass ihn leer",
  "the date format is longer than the {MAX_FORMAT_CHARS} characters a visible signature draws":
    "das Datumsformat ist länger als die {MAX_FORMAT_CHARS} Zeichen, die eine sichtbare Signatur zeichnet",
  "the date format, {format}, has characters tpdf cannot draw in a visible signature yet (it draws Latin-1 only)":
    "das Datumsformat, {format}, hat Zeichen, die tpdf in einer sichtbaren Signatur noch nicht zeichnen kann (es zeichnet nur Latin-1)",
  "the text has more than the {MAX_TEXT_LINES} lines a visible signature draws":
    "der Text hat mehr als die {MAX_TEXT_LINES} Zeilen, die eine sichtbare Signatur zeichnet",
  "a line of the text is longer than the {MAX_NOTE_CHARS} characters a visible signature draws":
    "eine Zeile des Textes ist länger als die {MAX_NOTE_CHARS} Zeichen, die eine sichtbare Signatur zeichnet",
  "the text, {line}, has characters tpdf cannot draw in a visible signature yet (it draws Latin-1 only), and drawing others would put different words on the page --- change the text":
    "der Text, {line}, hat Zeichen, die tpdf in einer sichtbaren Signatur noch nicht zeichnen kann (es zeichnet nur Latin-1), und andere zu zeichnen würde andere Wörter auf die Seite setzen - ändere den Text",
  "the text has no words in it, so there is nothing to draw": "der Text enthält keine Wörter, daher gibt es nichts zu zeichnen",
  "a visible signature has to show something --- choose an image or at least one line of text":
    "eine sichtbare Signatur muss etwas zeigen - wähle ein Bild oder mindestens eine Textzeile",
  "the certificate names nobody, so there is no name to draw":
    "das Zertifikat nennt niemanden, daher gibt es keinen Namen zu zeichnen",
  "the certificate's name is longer than the {MAX_NAME_CHARS} characters a visible signature draws --- sign without a visible appearance":
    "der Name des Zertifikats ist länger als die {MAX_NAME_CHARS} Zeichen, die eine sichtbare Signatur zeichnet - signiere ohne sichtbares Erscheinungsbild",
  "the certificate's name, {name}, has characters tpdf cannot draw in a visible signature yet (it draws Latin-1 only), and drawing others would put a different name on the page --- turn the name line off, or sign without a visible appearance":
    "der Name des Zertifikats, {name}, hat Zeichen, die tpdf in einer sichtbaren Signatur noch nicht zeichnen kann (es zeichnet nur Latin-1), und andere zu zeichnen würde einen anderen Namen auf die Seite setzen - schalte die Namenszeile aus oder signiere ohne sichtbares Erscheinungsbild",
  "the saved signature image is damaged": "das gespeicherte Unterschriftsbild ist beschädigt",
  "the rectangle chosen for the signature is not on its page":
    "das für die Signatur gewählte Rechteck liegt nicht auf seiner Seite",
  "the rectangle chosen for the signature is smaller than {MIN_SIDE} points a side, too small to read --- sign again and drag a larger one":
    "das für die Signatur gewählte Rechteck hat weniger als {MIN_SIDE} Punkt Seitenlänge und ist zu klein zum Lesen - signiere erneut und ziehe ein größeres auf",

  // sign_cms.rs
  "(a certificate with no readable name)": "(ein Zertifikat ohne lesbaren Namen)",
  "Save your changes first: a signature covers the file as it is on disk, and this document has edits that are not in it yet.":
    "Speichere zuerst deine Änderungen: Eine Signatur deckt die Datei so ab, wie sie auf dem Datenträger liegt, und dieses Dokument hat Änderungen, die dort noch nicht enthalten sind.",
  "This document is {mb} MB, and tpdf signs documents of up to {most} MB.":
    "Dieses Dokument ist {mb} MB groß, und tpdf signiert Dokumente bis {most} MB.",
  "its RSA key could not be read": "sein RSA-Schlüssel konnte nicht gelesen werden",
  "its RSA key has {bits} bits, and tpdf signs only with {MIN_RSA_BITS} or more":
    "sein RSA-Schlüssel hat {bits} Bit, und tpdf signiert nur mit {MIN_RSA_BITS} oder mehr",
  "its key is on an elliptic curve tpdf does not sign with":
    "sein Schlüssel liegt auf einer elliptischen Kurve, mit der tpdf nicht signiert",
  "its key is of a kind tpdf does not sign with (only RSA and ECDSA)":
    "sein Schlüssel ist von einer Art, mit der tpdf nicht signiert (nur RSA und ECDSA)",
  "it could not be read": "es konnte nicht gelesen werden",
  "it is not valid yet": "es ist noch nicht gültig",
  "it has expired": "es ist abgelaufen",
  "it is not issued for signing": "es ist nicht zum Signieren ausgestellt",
  "it is issued for {purposes}, not for signing documents": (a) =>
    `es ist für ${a
      .purposes!.split(" and ")
      .map((p) => PURPOSE[p] ?? p)
      .join(" und ")} ausgestellt, nicht zum Signieren von Dokumenten`,
  "its extended key usage could not be read": "seine erweiterte Schlüsselverwendung konnte nicht gelesen werden",
  "its key usage could not be read": "seine Schlüsselverwendung konnte nicht gelesen werden",
  "the signing certificate could not be read: {e}": "das Signaturzertifikat konnte nicht gelesen werden: {e}",
  "this certificate cannot sign: {why}": "dieses Zertifikat kann nicht signieren: {why}",
  "the signer could not be described: {e}": "der Unterzeichner konnte nicht beschrieben werden: {e}",
  "the signing certificate could not be named: {e}": "das Signaturzertifikat konnte nicht benannt werden: {e}",
  "the digest could not be named: {e}": "der Digest konnte nicht benannt werden: {e}",
  "the certificate could not be included: {e}": "das Zertifikat konnte nicht aufgenommen werden: {e}",
  "certificate {number} of the chain could not be read: {e}":
    "Zertifikat {number} der Kette konnte nicht gelesen werden: {e}",
  "the chain could not be included: {e}": "die Kette konnte nicht aufgenommen werden: {e}",
  "the signature could not be made: {e}": "die Signatur konnte nicht erstellt werden: {e}",
  "the signature could not be assembled: {e}": "die Signatur konnte nicht zusammengesetzt werden: {e}",
  "the signature could not be encoded: {e}": "die Signatur konnte nicht kodiert werden: {e}",
  "an ECDSA signature of {bytes} bytes has no halves": "eine ECDSA-Signatur aus {bytes} Bytes hat keine zwei Hälften",
  "the prepared signature {what}, so it was not built for the file on disk --- reopen the document and sign again":
    "die vorbereitete Signatur {what}, daher wurde sie nicht für die Datei auf dem Datenträger erstellt - öffne das Dokument erneut und signiere noch einmal",
  "was built against {built} bytes and the file has {was}": "wurde gegen {built} Bytes erstellt, und die Datei hat {was}",
  "covers a range that does not frame its own value": "deckt einen Bereich ab, der ihren eigenen Wert nicht einrahmt",
  "has something other than an empty value in its hole": "hat in ihrer Lücke etwas anderes als einen leeren Wert",
  "states a digest that is not the one of its own bytes": "nennt einen Digest, der nicht der ihrer eigenen Bytes ist",
  "the signature and its certificates take {bytes} bytes, and tpdf keeps signatures to {STEP_TWO_LIMIT} so a timestamp can be added later":
    "die Signatur und ihre Zertifikate brauchen {bytes} Bytes, und tpdf begrenzt Signaturen auf {STEP_TWO_LIMIT}, damit später ein Zeitstempel hinzugefügt werden kann",
  "the document timestamp's revision was built against {built_against} bytes, and the document is {len}":
    "die Revision des Dokumentzeitstempels wurde gegen {built_against} Bytes erstellt, und das Dokument hat {len}",
  "the document timestamp's revision covers a range that does not frame its own value":
    "die Revision des Dokumentzeitstempels deckt einen Bereich ab, der ihren eigenen Wert nicht einrahmt",
  "the document timestamp takes {bytes} bytes, and the space set aside for it holds {RESERVED}":
    "der Dokumentzeitstempel braucht {bytes} Bytes, und der dafür reservierte Platz fasst {RESERVED}",
  "tpdf's own check of the document timestamp it wrote did not call it intact ({verdict:?})":
    "tpdfs eigene Prüfung des geschriebenen Dokumentzeitstempels hat ihn nicht als intakt bewertet ({verdict})",
  "tpdf will not sign with this certificate: {why}": "tpdf signiert nicht mit diesem Zertifikat: {why}",
  "the signature just made does not have one signer": "die gerade erstellte Signatur hat nicht genau einen Unterzeichner",
  "the timestamp is not one DER value: {e}": "der Zeitstempel ist kein einzelner DER-Wert: {e}",
  "the timestamped signature could not be encoded: {e}": "die Signatur mit Zeitstempel konnte nicht kodiert werden: {e}",
  "the signature with its timestamp takes {bytes} bytes, and the space this document set aside for it holds {RESERVED}":
    "die Signatur mit ihrem Zeitstempel braucht {bytes} Bytes, und der Platz, den dieses Dokument dafür reserviert hat, fasst {RESERVED}",
  "tpdf's own check of the signature it just made did not find it intact ({verdict:?}, {why:?}), so nothing was written":
    "tpdfs eigene Prüfung der gerade erstellten Signatur hat sie nicht als intakt befunden ({verdict}, {why}), daher wurde nichts geschrieben",
  "tpdf's own check of the signature it just made did not find it intact ({verdict:?}), so nothing was written":
    "tpdfs eigene Prüfung der gerade erstellten Signatur hat sie nicht als intakt befunden ({verdict}), daher wurde nichts geschrieben",
  "tpdf's own check of the timestamp in the signature it just made did not find it intact ({verdict:?}, {why:?}), so nothing was written":
    "tpdfs eigene Prüfung des Zeitstempels in der gerade erstellten Signatur hat ihn nicht als intakt befunden ({verdict}, {why}), daher wurde nichts geschrieben",
  "tpdf's own check of the timestamp in the signature it just made did not find it intact ({verdict:?}), so nothing was written":
    "tpdfs eigene Prüfung des Zeitstempels in der gerade erstellten Signatur hat ihn nicht als intakt befunden ({verdict}), daher wurde nichts geschrieben",
  "the signature just made does not decode: {e}": "die gerade erstellte Signatur lässt sich nicht dekodieren: {e}",

  // longterm.rs
  "the signer's certificate ({cn})": "das Zertifikat des Unterzeichners ({cn})",
  "the certificate above the signer's ({cn})": "das Zertifikat über dem des Unterzeichners ({cn})",
  "the timestamp authority's certificate ({cn})": "das Zertifikat des Zeitstempeldienstes ({cn})",
  "the certificate above the timestamp authority's ({cn})": "das Zertifikat über dem des Zeitstempeldienstes ({cn})",
  "the signer's certificate": "das Zertifikat des Unterzeichners",
  "the timestamp authority's certificate": "das Zertifikat des Zeitstempeldienstes",
  "the signature just made could not be read to gather its data: {why}":
    "die gerade erstellte Signatur konnte nicht gelesen werden, um ihre Daten zu sammeln: {why}",
  "long-term validation data needs a timestamp: choose a timestamp authority too":
    "Langzeitvalidierungsdaten brauchen einen Zeitstempel: Wähle auch einen Zeitstempeldienst",
  "{name} does not say where its revocation data is published, so tpdf cannot add long-term validation data for it: that needs a certificate from a certificate authority that publishes revocation data (an OCSP responder or a revocation list over http or https). A timestamp alone works with this certificate":
    "{name} sagt nicht, wo seine Widerrufsdaten veröffentlicht werden, daher kann tpdf dafür keine Langzeitvalidierungsdaten hinzufügen: Dafür braucht es ein Zertifikat einer Zertifizierungsstelle, die Widerrufsdaten veröffentlicht (einen OCSP-Responder oder eine Widerrufsliste über http oder https). Ein Zeitstempel allein funktioniert mit diesem Zertifikat",
  "tpdf could not find the certificate that issued {name}, so it cannot ask about its revocation":
    "tpdf konnte das Zertifikat nicht finden, das {name} ausgestellt hat, und kann deshalb nicht nach seinem Widerruf fragen",
  "the timestamp authority {name} is not trusted by this computer, so tpdf will not fetch revocation data for it: {why}":
    "diesem Computer gilt der Zeitstempeldienst {name} nicht als vertrauenswürdig, daher holt tpdf keine Widerrufsdaten dafür: {why}",
  "tpdf could not get revocation data for {name}: {attempts}":
    "tpdf konnte keine Widerrufsdaten für {name} bekommen: {attempts}",
  "the OCSP responder at {host} says it does not know {name}, so there is no revocation data to keep for it":
    "der OCSP-Responder unter {host} sagt, dass er {name} nicht kennt, daher gibt es keine Widerrufsdaten, die dafür aufbewahrt werden könnten",
  "{name} has been revoked according to {by}. tpdf will not write a signature made with a revoked certificate": (a) =>
    `${a.name} ist laut ${dative(a.by!)} widerrufen. tpdf schreibt keine Signatur, die mit einem widerrufenen ` +
    "Zertifikat erstellt wurde",
  "{name} has been revoked since {at} according to {by}. tpdf will not write a signature made with a revoked certificate":
    (a) =>
      `${a.name} ist laut ${dative(a.by!)} seit ${a.at} widerrufen. tpdf schreibt keine Signatur, die mit einem ` +
      "widerrufenen Zertifikat erstellt wurde",
  "{by} about {name} does not check out: {why}": "{by} zu {name} hält der Prüfung nicht stand: {why}",
  "the long-term validation data {why}": "die Langzeitvalidierungsdaten {why}",
  "tpdf's own check of the signed document with its long-term validation data did not pass: {why}":
    "tpdfs eigene Prüfung des signierten Dokuments mit seinen Langzeitvalidierungsdaten ist fehlgeschlagen: {why}",
  "the archive timestamp, which keeps the signature checkable after the timestamp authority's own certificate expires, could not be added: {why}":
    "der Archivzeitstempel, der die Signatur prüfbar hält, wenn das eigene Zertifikat des Zeitstempeldienstes abgelaufen ist, konnte nicht hinzugefügt werden: {why}",
  "its certificate is not in force now": "sein Zertifikat ist jetzt nicht in Kraft",
  "whose certificate is not identified": "mit nicht bestimmtem Zertifikat",
  "its certificate is not identified": "ihr Zertifikat ist nicht bestimmt",
  "the timestamp's certificate is not identified": "das Zertifikat des Zeitstempels ist nicht bestimmt",
  "would follow a chain of certificates that loops back on itself, which no verifier can end at a root":
    "würden einer Zertifikatskette folgen, die auf sich selbst zurückführt und die kein Prüfprogramm an einem Stammzertifikat beenden kann",
  "would cover more than {MAX_SUBJECTS} certificates, more than any real chain has":
    "würden mehr als {MAX_SUBJECTS} Zertifikate abdecken, mehr als eine echte Kette hat",
  "the OCSP responder at {host} could not be reached ({why})":
    "der OCSP-Responder unter {host} war nicht erreichbar ({why})",
  "the revocation list at {host} could not be reached ({why})":
    "die Widerrufsliste unter {host} war nicht erreichbar ({why})",
  "the OCSP responder at {host} did not answer within {secs} seconds":
    "der OCSP-Responder unter {host} hat nicht innerhalb von {secs} Sekunden geantwortet",
  "the revocation list at {host} did not answer within {secs} seconds":
    "die Widerrufsliste unter {host} kam nicht innerhalb von {secs} Sekunden",
  "the OCSP responder at {host} answered with HTTP status {code}":
    "der OCSP-Responder unter {host} hat mit HTTP-Status {code} geantwortet",
  "the revocation list at {host} answered with HTTP status {code}":
    "die Widerrufsliste unter {host} kam mit HTTP-Status {code}",
  "the OCSP responder at {host} answered with more than {kib} KiB":
    "der OCSP-Responder unter {host} hat mit mehr als {kib} KiB geantwortet",
  "the revocation list at {host} answered with more than {kib} KiB":
    "die Widerrufsliste unter {host} war größer als {kib} KiB",
  "would take more than {MAX_REQUESTS} requests": "würden mehr als {MAX_REQUESTS} Anfragen brauchen",
  "took longer than {secs} seconds to gather": "haben zum Sammeln länger als {secs} Sekunden gebraucht",
  "would be more than {mib} MiB, more than tpdf adds to a document":
    "wären größer als {mib} MiB, mehr als tpdf einem Dokument hinzufügt",
  "the OCSP responder at {host} answered {status}": "der OCSP-Responder unter {host} hat mit {status} geantwortet",
  "the OCSP responder at {host} answered with something that is not an OCSP response":
    "der OCSP-Responder unter {host} hat mit etwas geantwortet, das keine OCSP-Antwort ist",
  "the OCSP response from {host}": "die OCSP-Antwort von {host}",
  "the revocation list from {host}": "die Widerrufsliste von {host}",
  "the new signature is not in it": "die neue Signatur ist nicht darin",
  "cannot be checked before it is written: the document's signatures together cover more data than tpdf checks at once, so the new signature was not checked again with the data added":
    "können nicht vor dem Schreiben geprüft werden: Die Signaturen des Dokuments erfassen zusammen mehr Daten, als tpdf auf einmal prüft, daher wurde die neue Signatur mit den hinzugefügten Daten nicht erneut geprüft",
  "the new signature does not read as intact": "die neue Signatur wird nicht als intakt gelesen",
  "the new signature's timestamp is not in it": "der Zeitstempel der neuen Signatur ist nicht darin",
  "the new signature's timestamp does not read as intact": "der Zeitstempel der neuen Signatur wird nicht als intakt gelesen",
  "no revocation answer for {whose}": "keine Widerrufsauskunft für {whose}",
  "the revocation list in the document": "die Widerrufsliste im Dokument",
  "the OCSP response in the document": "die OCSP-Antwort im Dokument",
  "no revocation answer for a chain": "keine Widerrufsauskunft für eine Kette",
  "the chain above {above} certificate is longer than tpdf judges": (a) =>
    `die Kette über dem Zertifikat ${owner(a.above!)} ist länger, als tpdf beurteilt`,
  "the worker built its revision against {built} bytes, and the signed document is {len}":
    "der Worker hat seine Revision gegen {built} Bytes erstellt, und das signierte Dokument hat {len}",
  "the archive timestamp's revision states a range outside itself":
    "die Revision des Archivzeitstempels nennt einen Bereich außerhalb ihrer selbst",

  // longterm/existing.rs
  "tpdf could not read this document's signatures: {why}": "tpdf konnte die Signaturen dieses Dokuments nicht lesen: {why}",
  "this document has no signature, so there is nothing to add long-term validation data for":
    "dieses Dokument hat keine Signatur, daher gibt es nichts, wofür Langzeitvalidierungsdaten hinzugefügt werden könnten",
  "tpdf could not read every signature in this document, so it cannot say that the data would cover them all":
    "tpdf konnte nicht jede Signatur in diesem Dokument lesen und kann deshalb nicht sagen, dass die Daten sie alle abdecken würden",
  "the signature {field} certifies this document with no changes permitted, and tpdf appends nothing to such a document: how readers judge validation data and a timestamp added after that certification has not been measured":
    "die Signatur {field} zertifiziert dieses Dokument ohne erlaubte Änderungen, und tpdf hängt an ein solches Dokument nichts an: Wie Prüfprogramme Validierungsdaten und einen Zeitstempel bewerten, die nach dieser Zertifizierung hinzukommen, ist nicht gemessen",
  "this document is certified with no changes permitted, and tpdf appends nothing to such a document: how readers judge validation data and a timestamp added after that certification has not been measured":
    "dieses Dokument ist ohne erlaubte Änderungen zertifiziert, und tpdf hängt an ein solches Dokument nichts an: Wie Prüfprogramme Validierungsdaten und einen Zeitstempel bewerten, die nach dieser Zertifizierung hinzukommen, ist nicht gemessen",
  "the timestamp of the signature {field} does not verify (it reads {reads}), and long-term validation data cannot make it verify":
    (a) =>
      `der Zeitstempel der Signatur ${a.field} hält der Prüfung nicht stand (er gilt als ${READS[a.reads!] ?? a.reads}), ` +
      "und Langzeitvalidierungsdaten können daran nichts ändern",
  "the signature {field} does not verify (it reads {reads}), and long-term validation data cannot make it verify": (a) =>
    `die Signatur ${a.field} hält der Prüfung nicht stand (sie gilt als ${READS[a.reads!] ?? a.reads}), ` +
    "und Langzeitvalidierungsdaten können daran nichts ändern",
  "this document's signatures together cover more data than tpdf checks at once, so it could not check the result before writing it":
    "die Signaturen dieses Dokuments erfassen zusammen mehr Daten, als tpdf auf einmal prüft, daher konnte es das Ergebnis vor dem Schreiben nicht prüfen",
  "the signer of {field} ({name}) is not trusted by this computer, so tpdf will not fetch revocation data for it: {why}":
    "diesem Computer gilt der Unterzeichner von {field} ({name}) nicht als vertrauenswürdig, daher holt tpdf keine Widerrufsdaten dafür: {why}",
  "this document's signatures name more than {MAX_SUBJECTS} certificates to ask about, more than tpdf asks about in one run":
    "die Signaturen dieses Dokuments nennen mehr als {MAX_SUBJECTS} Zertifikate, nach denen zu fragen wäre, mehr als tpdf in einem Durchgang fragt",
  "this document already carries as much validation data as tpdf reads, so tpdf could not read back what it would add":
    "dieses Dokument enthält schon so viele Validierungsdaten, wie tpdf liest, daher könnte tpdf nicht zurücklesen, was es hinzufügen würde",
  "for the signature {field}: {why}": "für die Signatur {field}: {why}",
  "tpdf's own check of the document with the data added did not pass: the signature {field} {what}":
    "tpdfs eigene Prüfung des Dokuments mit den hinzugefügten Daten ist fehlgeschlagen: Die Signatur {field} {what}",
  "tpdf's own check of the document with the data added did not pass: {why}":
    "tpdfs eigene Prüfung des Dokuments mit den hinzugefügten Daten ist fehlgeschlagen: {why}",
  "its value could not be read to find its certificates: {why}":
    "ihr Wert konnte nicht gelesen werden, um ihre Zertifikate zu finden: {why}",
  "{name} does not say where its revocation data is published (an OCSP responder or a revocation list over http or https), so there is none to add for it":
    "{name} sagt nicht, wo seine Widerrufsdaten veröffentlicht werden (ein OCSP-Responder oder eine Widerrufsliste über http oder https), daher gibt es keine, die dafür hinzugefügt werden könnten",
  "{name} has been revoked according to {by}, and tpdf adds long-term validation data only for certificates that are not revoked":
    (a) =>
      `${a.name} ist laut ${dative(a.by!)} widerrufen, und tpdf fügt Langzeitvalidierungsdaten nur für ` +
      "Zertifikate hinzu, die nicht widerrufen sind",
  "{name} has been revoked since {at} according to {by}, and tpdf adds long-term validation data only for certificates that are not revoked":
    (a) =>
      `${a.name} ist laut ${dative(a.by!)} seit ${a.at} widerrufen, und tpdf fügt Langzeitvalidierungsdaten nur ` +
      "für Zertifikate hinzu, die nicht widerrufen sind",
  "the worker's answer holds more signatures or certificates than a worker's may":
    "die Antwort des Workers enthält mehr Signaturen oder Zertifikate, als die Antwort eines Workers darf",
  "its timestamp's certificate is not identified": "das Zertifikat ihres Zeitstempels ist nicht bestimmt",
  "is not in it where it was": "steht nicht mehr dort, wo sie stand",
  "does not read as intact": "wird nicht als intakt gelesen",
  "is followed by a revision tpdf could not read": "wird von einer Revision gefolgt, die tpdf nicht lesen konnte",
  "reads as followed by a change to a page": "wird so gelesen, als folge ihr eine Änderung an einer Seite",
  "has a timestamp that does not read as intact": "hat einen Zeitstempel, der nicht als intakt gelesen wird",
  "no longer carries its timestamp": "trägt ihren Zeitstempel nicht mehr",
  "the archive timestamp is not in it": "der Archivzeitstempel ist nicht darin",
  "the archive timestamp does not cover the whole file": "der Archivzeitstempel deckt nicht die ganze Datei ab",
  "the archive timestamp does not read as intact": "der Archivzeitstempel wird nicht als intakt gelesen",
  "the worker built its revision against {built} bytes, and the document is {len}":
    "der Worker hat seine Revision gegen {built} Bytes erstellt, und das Dokument hat {len}",
  "the timestamp authority {name} is not trusted by this computer, so its timestamp would not keep the document checkable here: {why}":
    "diesem Computer gilt der Zeitstempeldienst {name} nicht als vertrauenswürdig, daher würde sein Zeitstempel das Dokument hier nicht prüfbar halten: {why}",

  // words.rs
  "a certificate between {whose} and a root is in neither the signature nor on {computer}, and tpdf does not look it up":
    (a) =>
      `ein Zertifikat zwischen dem ${owner(a.whose!)} und einem Stammzertifikat ist weder in der Signatur noch auf ` +
      `${computer(a.computer!, 1)}, und tpdf sucht es nicht`,
  "its chain ends at a root {computer} does not trust. A certificate somebody issued to themselves reads this way, and so does one whose root only Adobe's trust list carries":
    (a) =>
      `seine Kette endet an einem Stammzertifikat, dem ${computer(a.computer!, 0)} nicht vertraut. So liest sich ein ` +
      "Zertifikat, das sich jemand selbst ausgestellt hat, und ebenso eines, dessen Stammzertifikat nur die " +
      "Vertrauensliste von Adobe führt",
  "a certificate above {whose} is outside its dates": (a) =>
    `ein Zertifikat über dem ${owner(a.whose!)} liegt außerhalb seiner Gültigkeitsdaten`,
  "the signer's certificate was issued for something other than signing documents":
    "das Zertifikat des Unterzeichners wurde für etwas anderes als das Signieren von Dokumenten ausgestellt",
  "the authority's certificate was not issued for timestamping":
    "das Zertifikat des Zeitstempeldienstes wurde nicht für Zeitstempel ausgestellt",
  "{whose} certificate was not in force then": (a) => `das Zertifikat ${owner(a.whose!)} war damals nicht in Kraft`,
  "{computer} refused its chain": (a) => `${computer(a.computer!, 0)} hat seine Kette abgelehnt`,
  "the signature's certificates could not be prepared for the check":
    "die Zertifikate der Signatur konnten nicht für die Prüfung vorbereitet werden",
  "the operating system's trust check could not be run": "die Vertrauensprüfung des Betriebssystems konnte nicht laufen",
  "Added long-term validation data for {listed}, then the archive timestamp {archive} over the whole, and saved the result to {name}. Read back after writing: {each} intact, every certificate asked about reads as not revoked from the document's own data, and the archive timestamp covers the whole file.":
    (a) =>
      `Langzeitvalidierungsdaten für ${listed(a.listed!)} und danach der Archivzeitstempel ${a.archive} über das ` +
      `Ganze wurden hinzugefügt, und das Ergebnis wurde in ${a.name} gespeichert. Nach dem Schreiben ` +
      `zurückgelesen: ${EACH[a.each!] ?? a.each} intakt, jedes abgefragte Zertifikat gilt nach den eigenen Daten ` +
      "des Dokuments als nicht widerrufen, und der Archivzeitstempel deckt die ganze Datei ab.",

  // commands/sign.rs
  "the page chosen for the signature is not a page of the file on disk --- save the document and sign again":
    "die für die Signatur gewählte Seite ist keine Seite der Datei auf dem Datenträger - speichere das Dokument und signiere erneut",
  "the page chosen for the signature is no longer in the document":
    "die für die Signatur gewählte Seite ist nicht mehr im Dokument",
  "this signature is no longer held --- sign the document again":
    "diese Signatur wird nicht mehr bereitgehalten - signiere das Dokument erneut",
  "reading it back did not find the timestamp of the new signature {field} intact":
    "beim Zurücklesen war der Zeitstempel der neuen Signatur {field} nicht intakt",
  "reading it back did not find the new signature {field} locking the document":
    "beim Zurücklesen sperrte die neue Signatur {field} das Dokument nicht",
  "{name} was written, but {what}. Do not rely on that copy.":
    "{name} wurde geschrieben, aber {what}. Verlass dich nicht auf diese Kopie.",
  "the signed file was written and could not be reopened: {e}":
    "die signierte Datei wurde geschrieben und konnte nicht wieder geöffnet werden: {e}",
  "the signed file was written and could not be checked: {e}":
    "die signierte Datei wurde geschrieben und konnte nicht geprüft werden: {e}",
  "{why} --- nothing was written": "{why} - es wurde nichts geschrieben",
  "the certificate search did not run: {e}": "die Zertifikatssuche ist nicht gelaufen: {e}",
  "There is no home folder to keep the list of certificate files in.":
    "Es gibt keinen Benutzerordner, in dem die Liste der Zertifikatsdateien liegen könnte.",
  "{subject} can sign now: choose it when you sign a document.":
    "{subject} kann jetzt signieren: Wähle es, wenn du ein Dokument signierst.",
  "adding the certificate file did not run: {e}": "das Hinzufügen der Zertifikatsdatei ist nicht gelaufen: {e}",
  "Certificate files are added to tpdf on Linux; on macOS and Windows, import the file into your keychain or certificate store, and tpdf finds it there.":
    "Zertifikatsdateien werden unter Linux zu tpdf hinzugefügt; unter macOS und Windows importierst du die Datei in deinen Schlüsselbund oder Zertifikatspeicher, und tpdf findet sie dort.",
  "tpdf could not record what this file looked like when it was opened, so it cannot tell that what it signs is that file --- reopen it and sign again":
    "tpdf konnte nicht festhalten, wie diese Datei beim Öffnen aussah, und kann deshalb nicht sicherstellen, dass es genau diese Datei signiert - öffne sie erneut und signiere noch einmal",
  "the signing did not run: {e}": "das Signieren ist nicht gelaufen: {e}",

  // commands/validation.rs
  "{name} was written, but reading it back did not find what was added: {why}. Do not rely on that copy.":
    "{name} wurde geschrieben, aber beim Zurücklesen fand sich nicht, was hinzugefügt wurde: {why}. Verlass dich nicht auf diese Kopie.",
  "the copy was written and could not be reopened: {e}":
    "die Kopie wurde geschrieben und konnte nicht wieder geöffnet werden: {e}",
  "the copy was written and could not be checked: {e}": "die Kopie wurde geschrieben und konnte nicht geprüft werden: {e}",
  "This document is {mb} MB, and tpdf adds validation data to documents of up to {most} MB.":
    "Dieses Dokument ist {mb} MB groß, und tpdf fügt Validierungsdaten zu Dokumenten bis {most} MB hinzu.",
  "Save your changes first: validation data is added to the file as it is on disk, and this document has edits that are not in it yet.":
    "Speichere zuerst deine Änderungen: Validierungsdaten werden der Datei so hinzugefügt, wie sie auf dem Datenträger liegt, und dieses Dokument hat Änderungen, die dort noch nicht enthalten sind.",
  "The document with the data added is written as a new file --- choose a name other than the original's.":
    "Das Dokument mit den hinzugefügten Daten wird als neue Datei geschrieben - wähle einen anderen Namen als den des Originals.",
  "tpdf could not record what this file looked like when it was opened, so it cannot tell that what it adds to is that file --- reopen it and try again":
    "tpdf konnte nicht festhalten, wie diese Datei beim Öffnen aussah, und kann deshalb nicht sicherstellen, dass es genau diese Datei ergänzt - öffne sie erneut und versuche es noch einmal",
  "the file on disk is not the one that was opened, so nothing was added to it --- reopen it and try again":
    "die Datei auf dem Datenträger ist nicht die, die geöffnet wurde, daher wurde ihr nichts hinzugefügt - öffne sie erneut und versuche es noch einmal",
  "adding the validation data did not run: {e}": "das Hinzufügen der Validierungsdaten ist nicht gelaufen: {e}",
} satisfies Record<string, Wording>;
