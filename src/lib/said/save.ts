/** Saving, redaction and its verification, the page tree and merging:
 * `save.rs`, `save/`, `save_outside.rs`, `save_order.rs`, `redact.rs`,
 * `redact/glyph_cut.rs`, `raster_redact.rs`, `redaction_fill.rs`, `verify.rs`,
 * `pagetree.rs`, `merge.rs`, `sweep.rs`, `docgraph.rs`, `commands/redact.rs`
 * and `commands/save.rs`. */

import type { Wording } from "./de";

/** The German for a count: `one` when it is 1, `many` otherwise. */
const n = (count: string | undefined, one: string, many: string): string =>
  count === "1" ? one : many;

export default {
  // redact.rs: what stays, and why.
  "object {at} is a picture this document draws {times} time(s). It was taken off this page; it is still in the file, and on every other place that draws it":
    "Objekt {at} ist ein Bild, das dieses Dokument {times}-mal zeichnet. Es wurde von dieser Seite entfernt; es ist aber noch in der Datei und an jeder anderen Stelle, die es zeichnet",
  "object {at} is a drawing that reaches beyond the region and is not a straight line or a rectangle that can be cut at its edge, so it was left":
    "Objekt {at} ist eine Zeichnung, die über den Bereich hinausreicht und weder eine gerade Linie noch ein Rechteck ist, das sich an seinem Rand abschneiden lässt, daher blieb sie stehen",
  "object {at} is a drawing that also clips what is drawn after it, so it was left":
    "Objekt {at} ist eine Zeichnung, die auch alles danach Gezeichnete beschneidet, daher blieb sie stehen",
  "object {at} is a drawing inside the region that could not be found in the page's content by position, so it was left":
    "Objekt {at} ist eine Zeichnung im Bereich, die im Inhalt der Seite nicht anhand ihrer Position gefunden werden konnte, daher blieb sie stehen",
  "object {at} is text whose width could not be measured, and the text after it on its line starts where it ends. Removing it would move that text, so it was left":
    "Objekt {at} ist Text, dessen Breite nicht gemessen werden konnte, und der folgende Text in seiner Zeile beginnt dort, wo er endet. Ihn zu entfernen würde diesen Text verschieben, daher blieb er stehen",
  "object {at} is {what} drawn {times} time(s) in this document; removing it here would leave every other copy, and the object itself, in the file — so it was left":
    "Objekt {at} ({what}) wird in diesem Dokument {times}-mal gezeichnet; es hier zu entfernen würde jede andere Kopie und das Objekt selbst in der Datei lassen - daher blieb es stehen",
  "object {at} is {what} that overlaps the region; only text is removed here":
    "Objekt {at} ({what}) überlappt den Bereich; entfernt wird hier nur Text",
  "a picture": "ein Bild",
  "a drawing": "eine Zeichnung",
  "a colour gradient": "ein Farbverlauf",
  "a reusable block of content": "ein wiederverwendbarer Inhaltsblock",
  "something of a kind tpdf cannot read":
    "etwas, dessen Art tpdf nicht lesen kann",

  // redact.rs: reading and rewriting content streams.
  "the form's content stream could not be read: {why}":
    "Der Inhaltsstream des Form-XObjects konnte nicht gelesen werden: {why}",
  "the form's content stream will not decode: {why}":
    "Der Inhaltsstream des Form-XObjects lässt sich nicht dekodieren: {why}",
  "the form's content stream": "Der Inhaltsstream des Form-XObjects",
  "{what} does not parse to its end, and writing back the part that could be read would drop the rest of it. Nothing was removed.":
    "{what} lässt sich nicht bis zum Ende lesen, und nur den lesbaren Teil zurückzuschreiben würde den Rest verwerfen. Es wurde nichts entfernt.",
  "the region covers a marked-content span whose property list is the shared resource /{name}, and it carries /{key}. Editing it would change every other page that uses it, so nothing was removed.":
    "Der Bereich überdeckt einen Marked-Content-Abschnitt, dessen Eigenschaftsliste die gemeinsam genutzte Ressource /{name} ist, und sie enthält /{key}. Sie zu ändern würde jede andere Seite ändern, die sie nutzt, daher wurde nichts entfernt.",
  "the page names no content stream, so there was nothing to replace":
    "Die Seite nennt keinen Inhaltsstream, also gab es nichts zu ersetzen",
  "the page's content could not be replaced: {why}":
    "Der Inhalt der Seite konnte nicht ersetzt werden: {why}",
  "the page does not read back as the content that was written for it, so nothing can be said to have been removed":
    "Die Seite liest sich nicht als der Inhalt zurück, der für sie geschrieben wurde, daher lässt sich nicht sagen, dass etwas entfernt wurde",
  "the page's content stream could not be read: {why}":
    "Der Inhaltsstream der Seite konnte nicht gelesen werden: {why}",
  "the page's content stream": "Der Inhaltsstream der Seite",
  "the page has {count} text-showing operator(s) and PDFium reported {text_objects} text object(s). Removing by position needs those to agree, so nothing was removed.":
    "Die Seite hat {count} Textausgabe-Operator(en), und PDFium meldet {text_objects} Textobjekt(e). Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "there is no show operator {ordinal} on this page, which has {count}":
    "Auf dieser Seite gibt es keinen Textausgabe-Operator {ordinal}, sie hat {count}",
  "the rewritten content stream will not encode: {why}":
    "Der neu geschriebene Inhaltsstream lässt sich nicht kodieren: {why}",
  "object {at} is not one of the {count} form(s) on this page":
    "Objekt {at} gehört nicht zu den Form-XObjects dieser Seite ({count})",
  "the content stream will not decode: {why}":
    "Der Inhaltsstream lässt sich nicht dekodieren: {why}",
  "the page draws {draws} form XObject(s) and PDFium reported {reported}. Removing from one by position needs those to agree, so nothing was removed.":
    "Die Seite zeichnet {draws} Form-XObject(s), und PDFium meldet {reported}. Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "the text you marked is inside a form that this document draws {times} time(s). Removing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Der markierte Text liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Ihn zu entfernen würde jede dieser Stellen ändern, auch solche, die du nicht markiert hast, daher wurde nichts entfernt.",
  "this form has {count} text-showing operator(s) and PDFium reported {text_objects} text object(s). Removing by position needs those to agree, so nothing was removed.":
    "Dieses Form-XObject hat {count} Textausgabe-Operator(en), und PDFium meldet {text_objects} Textobjekt(e). Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "there is no show operator {ordinal} in this form, which has {count}":
    "In diesem Form-XObject gibt es keinen Textausgabe-Operator {ordinal}, es hat {count}",
  "the rewritten form stream will not encode: {why}":
    "Der neu geschriebene Stream des Form-XObjects lässt sich nicht kodieren: {why}",
  "the form's content stream could not be replaced: {why}":
    "Der Inhaltsstream des Form-XObjects konnte nicht ersetzt werden: {why}",
  "the rewritten form stream will not compress: {why}":
    "Der neu geschriebene Stream des Form-XObjects lässt sich nicht komprimieren: {why}",
  "the picture you marked is inside a form that this document draws {times} time(s). Removing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Das markierte Bild liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Es zu entfernen würde jede dieser Stellen ändern, auch solche, die du nicht markiert hast, daher wurde nichts entfernt.",
  "this form draws {count} picture(s) and PDFium reported {pictures}. Removing one by position needs those to agree, so nothing was removed.":
    "Dieses Form-XObject zeichnet {count} Bild(er), und PDFium meldet {pictures}. Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "there is no picture {ordinal} in this form, which draws {count}":
    "In diesem Form-XObject gibt es kein Bild {ordinal}, es zeichnet {count}",
  "the form could not be read: {why}":
    "Das Form-XObject konnte nicht gelesen werden: {why}",
  "the form's XObject list could not be read: {why}":
    "Die XObject-Liste des Form-XObjects konnte nicht gelesen werden: {why}",
  "the form's resources could not be read: {why}":
    "Die Ressourcen des Form-XObjects konnten nicht gelesen werden: {why}",
  "the page draws {count} image(s) and PDFium reported {image_objects}. Removing one by position needs those to agree, so nothing was removed.":
    "Die Seite zeichnet {count} Bild(er), und PDFium meldet {image_objects}. Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "there is no image {ordinal} on this page, which draws {count}":
    "Auf dieser Seite gibt es kein Bild {ordinal}, sie zeichnet {count}",
  "the page paints {count} path(s) and PDFium reported {path_objects}. Removing one by position needs those to agree, so nothing was removed.":
    "Die Seite zeichnet {count} Pfad(e), und PDFium meldet {path_objects}. Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "there is no path {ordinal} on this page, which paints {count}":
    "Auf dieser Seite gibt es keinen Pfad {ordinal}, sie zeichnet {count}",
  "a drawing you marked also clips what is drawn after it, and removing it would change the page outside the region — so nothing was removed.":
    "Eine markierte Zeichnung beschneidet auch alles danach Gezeichnete, und sie zu entfernen würde die Seite außerhalb des Bereichs ändern - daher wurde nichts entfernt.",
  "a drawing that was to be cut at the region's edge cannot be split exactly on this page — so nothing was removed.":
    "Eine Zeichnung, die am Rand des Bereichs abgeschnitten werden sollte, lässt sich auf dieser Seite nicht genau teilen - daher wurde nichts entfernt.",
  "the drawing you marked is inside a form that this document draws {times} time(s). Changing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Die markierte Zeichnung liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Sie zu ändern würde jede dieser Stellen ändern, auch solche, die du nicht markiert hast, daher wurde nichts entfernt.",
  "this form paints {count} path(s) and PDFium reported {expected}. Removing one by position needs those to agree, so nothing was removed.":
    "Dieses Form-XObject zeichnet {count} Pfad(e), und PDFium meldet {expected}. Entfernen nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "the page's resources name no XObject list to remove from":
    "Die Ressourcen der Seite nennen keine XObject-Liste, aus der entfernt werden könnte",
  "the page has no resource dictionary":
    "Die Seite hat kein Ressourcen-Dictionary",
  "the page's resources list no XObjects":
    "Die Ressourcen der Seite führen keine XObjects",
  "the form named {name} is written into the page rather than being its own object, which this cannot rewrite":
    "Das Form-XObject {name} ist direkt in die Seite geschrieben statt ein eigenes Objekt zu sein, und das lässt sich hier nicht umschreiben",
  "the page's resources do not name an XObject {name}":
    "Die Ressourcen der Seite nennen kein XObject {name}",

  // redact.rs: what the check after a redaction could and could not say.
  "the scan reads the whole file and could not say which page every word above is on, and this document holds {inserted} page(s) inserted from another document --- no region was marked on any of them and nothing in the write changed them, so a word reported above may be a second copy sitting on one of those pages":
    "Die Prüfung liest die ganze Datei und konnte nicht für jedes oben genannte Wort sagen, auf welcher Seite es steht, und dieses Dokument enthält {inserted} aus einem anderen Dokument eingefügte Seite(n) - auf keiner davon wurde ein Bereich markiert, und beim Schreiben wurde keine davon verändert. Ein oben genanntes Wort kann also eine zweite Kopie auf einer dieser Seiten sein",
  "{words} is too short to be told apart from the rest of the file: the check reads the whole file, where one or two characters also occur in the page's own numbers and other text, so it cannot say whether the marked one was removed --- mark the whole word or number to get an answer":
    "{words} ist zu kurz, um es vom Rest der Datei zu unterscheiden: Die Prüfung liest die ganze Datei, in der ein oder zwei Zeichen auch in den Zahlen und im übrigen Text der Seite vorkommen, daher kann sie nicht sagen, ob das Markierte entfernt wurde - markiere das ganze Wort oder die ganze Zahl, um eine Antwort zu bekommen",
  "a word reported above is still on {pages}, where regions were marked for removal, so the removal did not take it there":
    "Ein oben genanntes Wort steht noch auf {pages}, wo Bereiche zum Entfernen markiert waren, das Entfernen hat es dort also nicht erfasst",
  "every word reported above is on a page no region was marked on":
    "Jedes oben genannte Wort steht auf einer Seite, auf der kein Bereich markiert war",
  "every word reported above is on a page no region was marked on --- {pages} carried the regions, and none of them carries any of those words":
    "Jedes oben genannte Wort steht auf einer Seite, auf der kein Bereich markiert war - die Bereiche lagen auf {pages}, und dort steht keines dieser Wörter",

  // redact/glyph_cut.rs
  "a line of text is not shaped the way it was when it was measured, so nothing was removed":
    "Eine Textzeile ist nicht mehr so aufgebaut wie beim Messen, daher wurde nichts entfernt",
  "a line of text was not measured to the show after it, so nothing was removed":
    "Eine Textzeile wurde nicht bis zur folgenden Textausgabe gemessen, daher wurde nichts entfernt",
  "the page has {count} text-showing operator(s) and PDFium reported {text_objects} text object(s). Cutting by position needs those to agree, so nothing was removed.":
    "Die Seite hat {count} Textausgabe-Operator(en), und PDFium meldet {text_objects} Textobjekt(e). Schneiden nach Position setzt voraus, dass beides übereinstimmt, daher wurde nichts entfernt.",
  "show operator {ordinal} is cut twice, so nothing was removed":
    "Textausgabe-Operator {ordinal} wird zweimal geschnitten, daher wurde nichts entfernt",
  "a line of text is in a font whose codes this cannot count, so nothing was removed":
    "Eine Textzeile steht in einer Schrift, deren Zeichencodes sich hier nicht zählen lassen, daher wurde nichts entfernt",

  // raster_redact.rs: the image-only copy.
  "raster document could not be read: {e:?}":
    "Das Rasterdokument konnte nicht gelesen werden: {e}",
  "raster redaction requires finite positive page dimensions":
    "Die Schwärzung als Bild braucht endliche, positive Seitenmaße",
  "page exceeds the raster redaction pixel limit":
    "Die Seite überschreitet die Pixelgrenze der Schwärzung als Bild",
  "missing region corners": "Die Ecken des Bereichs fehlen",
  "a marked region lies outside its output page":
    "Ein markierter Bereich liegt außerhalb seiner Ausgabeseite",
  "a marked region has invalid or zero dimensions":
    "Ein markierter Bereich hat ungültige Maße oder die Größe null",
  "PDFium could not locate a marked region":
    "PDFium konnte einen markierten Bereich nicht finden",
  "a raster redaction render did not complete":
    "Das Rendern für die Schwärzung als Bild wurde nicht fertig",
  "a raster redaction pixel was not opaque":
    "Ein Pixel der Schwärzung als Bild war nicht deckend",
  "could not read raster PDF: {e}":
    "Das Raster-PDF konnte nicht gelesen werden: {e}",
  "raster output could not be unlocked for verification":
    "Die Rasterausgabe konnte für die Prüfung nicht entsperrt werden",
  "unexpected raster output trailer":
    "Unerwarteter Trailer in der Rasterausgabe",
  "raster output object inventory changed: expected {expected}, read {read}":
    "Der Objektbestand der Rasterausgabe hat sich geändert: erwartet {expected}, gelesen {read}",
  "unexpected raster image carrier":
    "Unerwarteter Bildträger in der Rasterausgabe",
  "raster image geometry or encoding changed":
    "Geometrie oder Kodierung eines Rasterbilds hat sich geändert",
  "raster pixels changed during serialization":
    "Rasterpixel haben sich beim Schreiben geändert",
  "raster drawing commands changed":
    "Die Zeichenbefehle der Rasterausgabe haben sich geändert",
  "raster page graph changed":
    "Der Seitengraph der Rasterausgabe hat sich geändert",
  "save text edits before applying redactions":
    "Speichere die Textänderungen, bevor du Schwärzungen anwendest",
  "no complete redaction regions were supplied":
    "Es wurden keine vollständigen Schwärzungsbereiche übergeben",
  "raster redaction page limit exceeded":
    "Die Seitengrenze der Schwärzung als Bild ist überschritten",
  "An image-only copy is drawn from this document's own pages, and some of these came from another document. Redact without the image-only option, or save this document, reopen it, and make an image-only copy there.":
    "Eine Kopie nur aus Bildern entsteht aus den eigenen Seiten dieses Dokuments, und einige davon stammen aus einem anderen Dokument. Schwärze ohne die Option „Kopie nur aus Bildern“ oder speichere dieses Dokument, öffne es neu und erstelle die Kopie nur aus Bildern dort.",
  "working PDF exceeds raster redaction size limit":
    "Das Arbeits-PDF überschreitet die Größengrenze der Schwärzung als Bild",
  "cannot preserve an encryption policy without its key":
    "Eine Verschlüsselung lässt sich ohne ihren Schlüssel nicht erhalten",
  "raster working page count disagrees with the plan":
    "Die Seitenzahl des Arbeits-PDFs passt nicht zum Plan",
  "a marked source does not name exactly one output page":
    "Eine markierte Quellseite nennt nicht genau eine Ausgabeseite",
  "raster redaction exceeded its time limit":
    "Die Schwärzung als Bild hat ihr Zeitlimit überschritten",
  "raster size overflow": "Überlauf bei der Rastergröße",
  "document exceeds raster redaction pixel budget":
    "Das Dokument überschreitet das Pixelbudget der Schwärzung als Bild",
  "raster stream size overflow": "Überlauf bei der Größe der Rasterstreams",
  "compressed raster images exceed the memory budget":
    "Die komprimierten Rasterbilder überschreiten das Speicherbudget",
  "could not preserve raster output encryption: {e}":
    "Die Verschlüsselung der Rasterausgabe konnte nicht erhalten werden: {e}",
  "raster output exceeds the file size limit":
    "Die Rasterausgabe überschreitet die Dateigrößengrenze",
  "PDFium reported a different raster output page count":
    "PDFium meldet eine andere Seitenzahl für die Rasterausgabe",
  "raster output page geometry or text layer is inconsistent":
    "Seitengeometrie oder Textebene der Rasterausgabe ist widersprüchlich",
  "raster verification exceeded its time limit":
    "Die Prüfung der Rasterausgabe hat ihr Zeitlimit überschritten",
  "PDFium found pixels of another colour than the fill inside a redacted region":
    "PDFium hat in einem geschwärzten Bereich Pixel in einer anderen Farbe als der Füllung gefunden",

  // redaction_fill.rs
  "Too many pages for the redaction fill":
    "Zu viele Seiten für die Füllung der Schwärzung",
  "The redaction fill addresses a missing page":
    "Die Füllung der Schwärzung nennt eine Seite, die es nicht gibt",
  "The redaction fill has an invalid rectangle":
    "Die Füllung der Schwärzung hat ein ungültiges Rechteck",

  // verify.rs
  "pages {pages}, and {more} more": "Seiten {pages} und {more} weiteren",
  "no page": "keiner Seite",
  "pages {first} and {last}": "Seiten {first} und {last}",
  "{needle} is still in the file": "{needle} ist noch in der Datei",
  "{needle} is still in the file, on {pages}":
    "{needle} ist noch in der Datei, auf {pages}",
  "{needle} is still in the file, carried by something more than one page draws ({pages}), so which page it is on could not be established":
    "{needle} ist noch in der Datei, in etwas, das mehr als eine Seite zeichnet ({pages}), daher ließ sich nicht feststellen, auf welcher Seite es steht",
  "{needle} is still in the file, in something no page of it reaches, so which page it is on could not be established":
    "{needle} ist noch in der Datei, in etwas, das keine ihrer Seiten erreicht, daher ließ sich nicht feststellen, auf welcher Seite es steht",
  "the file does not begin with a PDF header":
    "Die Datei beginnt nicht mit einem PDF-Header",
  "the file has no %%EOF marker": "Die Datei hat keine %%EOF-Markierung",
  "the file has a %%EOF marker and none that ends a revision: none follows a startxref and its offset":
    "Die Datei hat eine %%EOF-Markierung, aber keine, die eine Revision abschließt: Keine folgt auf ein startxref mit seinem Offset",
  "the file has {many} %%EOF markers, so it holds more than one revision — a rewrite writes exactly one, and an earlier revision is content no parser will show and no scan can decode":
    "Die Datei hat {many} %%EOF-Markierungen, enthält also mehr als eine Revision - ein Neuschreiben schreibt genau eine, und eine frühere Revision ist Inhalt, den kein Parser anzeigt und keine Prüfung dekodieren kann",
  "{trailing} byte(s) follow the last %%EOF, which belong to no object and which nothing here put there":
    (a) =>
      `Nach dem letzten %%EOF ${n(a.trailing, "folgt", "folgen")} ${a.trailing} ${n(a.trailing, "Byte", "Bytes")}, die zu keinem Objekt gehören und die nichts hier dort abgelegt hat`,
  "the file has no startxref": "Die Datei hat kein startxref",
  "the file's startxref has no offset after it":
    "Auf das startxref der Datei folgt kein Offset",
  "startxref points at byte {offset} of a {size}-byte file":
    "startxref zeigt auf Byte {offset} einer Datei mit {size} Bytes",
  "the file has {count} %%EOF markers, so earlier revisions exist that no parser will resolve and no scan can decode":
    "Die Datei hat {count} %%EOF-Markierungen, es gibt also frühere Revisionen, die kein Parser auflöst und keine Prüfung dekodieren kann",
  "the file could not be parsed, so nothing in it was accounted for: {why}":
    "Die Datei konnte nicht gelesen werden, daher wurde nichts darin erfasst: {why}",
  "the file is encrypted and no password opened it, so nothing in it was decoded and nothing it contains was checked":
    "Die Datei ist verschlüsselt und kein Passwort hat sie geöffnet, daher wurde nichts darin dekodiert und nichts darin geprüft",
  "the file parsed to no objects at all, so nothing in it was accounted for":
    "Die Datei enthält beim Lesen überhaupt keine Objekte, daher wurde nichts darin erfasst",
  "object {object} is a {filter} image, so its encoded bytes were scanned and its picture was not read — text visible only as pixels needs OCR":
    "Objekt {object} ist ein {filter}-Bild, daher wurden seine kodierten Bytes geprüft, das Bild selbst aber nicht gelesen - Text, der nur als Pixel sichtbar ist, braucht Texterkennung",
  "object {object} is {filter}, which this build cannot decode, so its contents are unknown":
    "Objekt {object} ist {filter}, das diese Version nicht dekodieren kann, daher ist sein Inhalt unbekannt",
  "object {object} uses {filter}, which nothing here recognises":
    "Objekt {object} verwendet {filter}, das hier nichts erkennt",
  "object {object} could not be decoded, so its contents are unknown: {why}":
    "Objekt {object} konnte nicht dekodiert werden, daher ist sein Inhalt unbekannt: {why}",
  "{count} further images were not read, and are not listed individually":
    "{count} weitere Bilder wurden nicht gelesen und werden nicht einzeln aufgeführt",
  "{count} further objects could not be accounted for, and are not listed individually":
    "{count} weitere Objekte konnten nicht erfasst werden und werden nicht einzeln aufgeführt",

  // save.rs
  "{reason} A print job is built from the file on disk, so the paper would carry that document with your edits placed on the pages of the one you opened.":
    "{reason} Ein Druckauftrag entsteht aus der Datei auf dem Datenträger, das Papier zeigte also jenes Dokument, mit deinen Änderungen auf den Seiten des Dokuments, das du geöffnet hast.",
  "the print job needs somewhere to be built and {at:?} could not be created: {e}":
    "Der Druckauftrag braucht einen Ort, an dem er entsteht, und {at} konnte nicht angelegt werden: {e}",
  "could not find an unused temporary name in {dir:?} after {STAGING_ATTEMPTS} tries":
    "In {dir} wurde nach {STAGING_ATTEMPTS} Versuchen kein freier temporärer Name gefunden",
  "tpdf cannot save over the document it is reading --- choose another name":
    "tpdf kann nicht über das Dokument speichern, das es gerade liest - wähle einen anderen Namen",
  "Choose a different name for the image-only copy; the original must be kept":
    "Wähle einen anderen Namen für die Kopie nur aus Bildern; das Original muss erhalten bleiben",
  "The source could not be fingerprinted. Reopen it before redacting":
    "Von der Quelle ließ sich kein Fingerabdruck bilden. Öffne sie neu, bevor du schwärzt",
  "The source changed since it was opened. Reopen it and mark the regions again":
    "Die Quelle hat sich seit dem Öffnen geändert. Öffne sie neu und markiere die Bereiche noch einmal",
  "The redaction output changed before its fill was written":
    "Die Ausgabe der Schwärzung hat sich geändert, bevor ihre Füllung geschrieben wurde",
  "a split writes at least two files":
    "Ein Aufteilen schreibt mindestens zwei Dateien",
  "{path} already exists, and a split would replace it --- choose another name":
    "{path} gibt es schon, und das Aufteilen würde es ersetzen - wähle einen anderen Namen",
  "{reason} ({written} of {total} written)":
    "{reason} ({written} von {total} geschrieben)",
  "choose at least one document to merge in":
    "Wähle mindestens ein Dokument zum Zusammenführen",
  "tpdf cannot write the merge over {name}, which is one of the documents going into it":
    "tpdf kann das Zusammengeführte nicht über {name} schreiben, das selbst eines der zusammengeführten Dokumente ist",
  "choose at least one picture": "Wähle mindestens ein Bild",
  "tpdf cannot write the document over {name}, which is one of the pictures going into it":
    "tpdf kann das Dokument nicht über {name} schreiben, das eines der Bilder darin ist",
  "the pictures come to {total} MB, and tpdf takes at most {limit} MB at once --- make the document in two passes and merge them":
    "Die Bilder sind zusammen {total} MB groß, und tpdf nimmt höchstens {limit} MB auf einmal - erstelle das Dokument in zwei Durchgängen und führe sie zusammen",
  "the document": "das Dokument",
  "could not measure {source}: {e}":
    "Die Größe von {source} konnte nicht ermittelt werden: {e}",
  "the documents to merge come to at least {total} MB, and tpdf merges at most {limit} MB at once --- merge fewer files, or merge them in two passes":
    "Die zusammenzuführenden Dokumente sind zusammen mindestens {total} MB groß, und tpdf führt höchstens {limit} MB auf einmal zusammen - führe weniger Dateien zusammen oder arbeite in zwei Durchgängen",
  "{label} is larger than this machine can map":
    "{label} ist größer, als dieser Rechner einblenden kann",
  "the documents to merge are larger than this machine can map":
    "Die zusammenzuführenden Dokumente sind größer, als dieser Rechner einblenden kann",
  "tpdf could not record what this file looked like when it was opened, so it cannot tell whether saving over it is safe --- use Save a copy":
    "tpdf konnte nicht festhalten, wie diese Datei beim Öffnen aussah, und kann daher nicht sagen, ob Speichern darüber sicher ist - nutze „Kopie speichern“",
  "tpdf could not confirm what this file was when it read it --- use Save a copy":
    "tpdf konnte nicht bestätigen, welche Datei es beim Lesen vor sich hatte - nutze „Kopie speichern“",
  "tpdf did not record what {label} was when its pages were inserted, so it cannot tell whether they are still those pages --- undo the insert and insert them again":
    "tpdf hat nicht festgehalten, wie {label} beim Einfügen seiner Seiten aussah, und kann daher nicht sagen, ob es noch dieselben Seiten sind - mach das Einfügen rückgängig und füge sie noch einmal ein",
  "{label} has changed since its pages were inserted, so they may no longer be the pages you placed --- undo the insert and insert them again":
    "{label} hat sich seit dem Einfügen seiner Seiten geändert, es sind also vielleicht nicht mehr die Seiten, die du eingefügt hast - mach das Einfügen rückgängig und füge sie noch einmal ein",
  "could not measure the staged file: {e}":
    "Die Größe der vorbereiteten Datei konnte nicht ermittelt werden: {e}",
  "the rewritten document was reported as {wrote} bytes and the staged file is {landed} --- the save was not completed":
    "Das neu geschriebene Dokument wurde mit {wrote} Bytes gemeldet, die vorbereitete Datei hat {landed} - das Speichern wurde nicht abgeschlossen",
  "could not open {source:?} to read it: {e}":
    "{source} konnte zum Lesen nicht geöffnet werden: {e}",
  "this document needs a full rewrite rather than an append":
    "Dieses Dokument muss ganz neu geschrieben werden, ein Anhängen reicht nicht",
  "these edits were built against {built} bytes and the file is {size} --- reopen it before saving":
    "Diese Änderungen beziehen sich auf {built} Bytes, die Datei hat {size} - öffne sie vor dem Speichern neu",
  "This document is encrypted and tpdf could not unlock it, so it cannot be saved. Open it with its password first.":
    "Dieses Dokument ist verschlüsselt und tpdf konnte es nicht entsperren, daher kann es nicht gespeichert werden. Öffne es zuerst mit seinem Passwort.",
  "the document on disk has {pages} page(s) and the edits were made against {baseline} --- it has changed since it was opened, so reopen it before saving":
    "Das Dokument auf dem Datenträger hat {pages} Seite(n), die Änderungen beziehen sich auf {baseline} - es hat sich seit dem Öffnen geändert, öffne es also vor dem Speichern neu",
  "a mark names page {page}, which this document does not have":
    "Eine Markierung nennt Seite {page}, die es in diesem Dokument nicht gibt",
  "a save that only adds marks cannot carry a page tpdf made":
    "Ein Speichern, das nur Markierungen hinzufügt, kann keine von tpdf erstellte Seite mitnehmen",
  "a save that only adds marks cannot carry a page from another document":
    "Ein Speichern, das nur Markierungen hinzufügt, kann keine Seite aus einem anderen Dokument mitnehmen",
  "could not bring this page's /Annots across: {e}":
    "Die /Annots dieser Seite ließen sich nicht übernehmen: {e}",
  "could not bring this page across: {e}":
    "Diese Seite ließ sich nicht übernehmen: {e}",
  "could not build the update section: {e}":
    "Der Aktualisierungsabschnitt konnte nicht erstellt werden: {e}",
  "the print job could not be read back: {e}":
    "Der Druckauftrag konnte nicht zurückgelesen werden: {e}",
  "the rewritten document could not be written: {e}":
    "Das neu geschriebene Dokument konnte nicht gespeichert werden: {e}",
  "the print job could not be written: {e}":
    "Der Druckauftrag konnte nicht geschrieben werden: {e}",
  "the merged document could not be written: {e}":
    "Das zusammengeführte Dokument konnte nicht geschrieben werden: {e}",
  "the document could not be written: {e}":
    "Das Dokument konnte nicht geschrieben werden: {e}",
  "the attachment could not be written: {e}":
    "Der Anhang konnte nicht geschrieben werden: {e}",
  "this change has to rewrite object {object} of the document, and that object is marked as part of the file's own structure (/{kind}): the update would be written without it and look complete, so tpdf does not write one":
    "Diese Änderung müsste Objekt {object} des Dokuments neu schreiben, und dieses Objekt ist als Teil der Dateistruktur gekennzeichnet (/{kind}): Die Aktualisierung würde ohne es geschrieben und sähe trotzdem vollständig aus, daher schreibt tpdf keine",
  "this document describes one of its own objects, number {object}, as part of the file's layout (/{kind}): a copy would be written without it and look complete, so tpdf does not write one":
    "Dieses Dokument beschreibt eines seiner eigenen Objekte, Nummer {object}, als Teil des Dateiaufbaus (/{kind}): Eine Kopie würde ohne es geschrieben und sähe trotzdem vollständig aus, daher schreibt tpdf keine",
  "{why} --- nothing was written, so the file on disk is untouched. Your document has been closed and the edits you had made are gone.":
    "{why} - es wurde nichts geschrieben, die Datei auf dem Datenträger ist also unverändert. Dein Dokument wurde geschlossen, und deine Änderungen sind verloren.",
  "could not open {source:?} to save it: {e}":
    "{source} konnte zum Speichern nicht geöffnet werden: {e}",
  "tpdf could not tell which file {source:?} is, so it cannot promise a save would land there --- nothing was written. Use Save a copy.":
    "tpdf konnte nicht feststellen, welche Datei {source} ist, und kann daher nicht zusichern, dass ein Speichern dort ankommt - es wurde nichts geschrieben. Nutze „Kopie speichern“.",
  "{why} --- nothing was written, so the file on disk is untouched":
    "{why} - es wurde nichts geschrieben, die Datei auf dem Datenträger ist also unverändert",
  "{why} --- the file has been put back as it was":
    "{why} - die Datei wurde wiederhergestellt, wie sie war",
  "{why} --- and it could not be put back: {also}":
    "{why} - und sie konnte nicht wiederhergestellt werden: {also}",
  "could not append to {source:?}: {e}":
    "An {source} konnte nichts angehängt werden: {e}",
  "the saved file has {pages} page(s) and should have {expected}":
    "Die gespeicherte Datei hat {pages} Seite(n), sollte aber {expected} haben",
  "the saved file could not be read back: {e}":
    "Die gespeicherte Datei konnte nicht zurückgelesen werden: {e}",
  "{source:?} stopped being the file it was while it was being saved --- something else renamed or removed it. The edits were written to the file that had that name when the save began, and nothing that has the name now was touched.":
    "{source} war während des Speicherns nicht mehr dieselbe Datei - etwas anderes hat sie umbenannt oder entfernt. Die Änderungen wurden in die Datei geschrieben, die zu Beginn des Speicherns so hieß, und was jetzt so heißt, wurde nicht angerührt.",
  "Image-only redaction requires the sandboxed rendering worker":
    "Die Schwärzung als Bild braucht den abgeschotteten Render-Arbeitsprozess",
  "This document is encrypted, and printing part of it would have to write a copy the printer could not read. Print the whole document instead --- that is handed over unchanged.":
    "Dieses Dokument ist verschlüsselt, und einen Teil davon zu drucken würde eine Kopie erfordern, die der Drucker nicht lesen könnte. Drucke stattdessen das ganze Dokument - das wird unverändert übergeben.",
  "{label} is named as {length} bytes at {offset} in the merge, and only {handed} were handed over":
    "{label} ist beim Zusammenführen mit {length} Bytes an Position {offset} angegeben, übergeben wurden aber nur {handed}",
  "a merge was asked for with no documents to merge in":
    "Ein Zusammenführen wurde ohne Dokumente zum Zusammenführen angefordert",
  "save this document before merging it --- it holds pages inserted from another document, and a merge cannot carry those yet":
    "Speichere dieses Dokument, bevor du es zusammenführst - es enthält aus einem anderen Dokument eingefügte Seiten, und ein Zusammenführen kann die noch nicht mitnehmen",
  "tpdf could not read back the document it just built: {e}":
    "tpdf konnte das gerade erstellte Dokument nicht zurücklesen: {e}",
  "{label} is encrypted, and merging rewrites it --- which would silently remove that. Leave it out, or save an unencrypted copy of it first.":
    "{label} ist verschlüsselt, und das Zusammenführen schreibt es neu - dabei ginge die Verschlüsselung stillschweigend verloren. Lass es weg oder speichere zuerst eine unverschlüsselte Kopie davon.",
  "could not merge {label}: {why}":
    "{label} konnte nicht zusammengeführt werden: {why}",
  "tpdf could not restore this document's encryption: {e}":
    "tpdf konnte die Verschlüsselung dieses Dokuments nicht wiederherstellen: {e}",
  "the merged document": "das zusammengeführte Dokument",
  "a document must keep at least one page":
    "Ein Dokument muss mindestens eine Seite behalten",
  "This document is encrypted and tpdf could not unlock it, so it cannot be rewritten. Open it with its password first.":
    "Dieses Dokument ist verschlüsselt und tpdf konnte es nicht entsperren, daher kann es nicht neu geschrieben werden. Öffne es zuerst mit seinem Passwort.",
  "the edits name page {page}, which this document does not have":
    "Die Änderungen nennen Seite {page}, die es in diesem Dokument nicht gibt",
  "the edits ask for a page {width} by {height} points, which is not a page this can write":
    "Die Änderungen verlangen eine Seite von {width} mal {height} Punkt, und eine solche Seite kann hier nicht geschrieben werden",
  "the edits place a page from document {document}, which the save was not given":
    "Die Änderungen setzen eine Seite aus Dokument {document} ein, das dem Speichern nicht übergeben wurde",
  "the edits place page {page} of a document with {has} page(s)":
    "Die Änderungen setzen Seite {page} eines Dokuments mit {has} Seite(n) ein",
  "the edits place pages from another document, and that document was not handed to the save":
    "Die Änderungen setzen Seiten aus einem anderen Dokument ein, und dieses Dokument wurde dem Speichern nicht übergeben",
  "the edits place pages from {count} other document(s), and the save was handed {handed}":
    "Die Änderungen setzen Seiten aus {count} anderen Dokument(en) ein, übergeben wurden dem Speichern {handed}",
  "{label} is encrypted, and inserting its pages would write them without that --- save an unencrypted copy of it and insert from that instead":
    "{label} ist verschlüsselt, und beim Einfügen seiner Seiten würden sie ohne Verschlüsselung geschrieben - speichere eine unverschlüsselte Kopie davon und füge stattdessen aus dieser ein",
  "the edits change text in a document this save was not given":
    "Die Änderungen ändern Text in einem Dokument, das diesem Speichern nicht übergeben wurde",
  "page {page} is named twice by the redaction plan":
    "Seite {page} ist im Schwärzungsplan zweimal genannt",
  "a redaction names page {page} of a document that has {count}":
    "Eine Schwärzung nennt Seite {page} eines Dokuments mit {count} Seiten",
  "could not serialise {what}: {e}":
    "{what} konnte nicht geschrieben werden: {e}",
  "tpdf built {what} and then found it malformed, so nothing was written: {problems}":
    "tpdf hat {what} erstellt und danach fehlerhaft vorgefunden, daher wurde nichts geschrieben: {problems}",
  "pages {gone} and {shared} are the same page in this file, so page {again} cannot be removed on its own. Remove both, or keep both.":
    "Die Seiten {gone} und {shared} sind in dieser Datei dieselbe Seite, daher kann Seite {again} nicht allein entfernt werden. Entferne beide oder behalte beide.",
  "page {page} of the edits was not imported":
    "Seite {page} der Änderungen wurde nicht importiert",
  "the edits change text on a page from document {file}, which the save was not given":
    "Die Änderungen ändern Text auf einer Seite aus Dokument {file}, das dem Speichern nicht übergeben wurde",
  "the edits change text on page {page} of another document, which this save does not place":
    "Die Änderungen ändern Text auf Seite {page} eines anderen Dokuments, die dieses Speichern nicht einsetzt",
  "page {page} of another document is in this one {placed} times, and editing its text would change every one of them":
    "Seite {page} eines anderen Dokuments steht {placed}-mal in diesem, und ihren Text zu ändern würde jede davon ändern",
  "could not insert the pages from another document: {why}":
    "Die Seiten aus einem anderen Dokument konnten nicht eingefügt werden: {why}",
  "pages {first} and {second} are the same page in this file, so they cannot be cropped differently. Crop them the same way, or leave both as they are.":
    "Die Seiten {first} und {second} sind in dieser Datei dieselbe Seite und lassen sich daher nicht unterschiedlich zuschneiden. Schneide beide gleich zu oder lass beide, wie sie sind.",
  "{out:?} does not name a file to save to":
    "{out} nennt keine Datei, in die gespeichert werden kann",
  "could not create {partial:?}: {e}":
    "{partial} konnte nicht angelegt werden: {e}",
  "could not flush {partial:?}: {e}":
    "{partial} konnte nicht vollständig geschrieben werden: {e}",
  "could not find an unused temporary name beside {out:?} after {STAGING_ATTEMPTS} tries":
    "Neben {out} wurde nach {STAGING_ATTEMPTS} Versuchen kein freier temporärer Name gefunden",
  "could not put {out:?} in place: {e}":
    "{out} konnte nicht an seinen Platz gebracht werden: {e}",

  // save/marks.rs
  "page {page} is the same page object as another page in this file, so a mark on it would appear on both. tpdf will not write it.":
    "Seite {page} ist in dieser Datei dasselbe Seitenobjekt wie eine andere Seite, eine Markierung darauf erschiene also auf beiden. tpdf schreibt sie nicht.",
  "page {page:?} is not a dictionary: {e}":
    "Seite {page} ist kein Dictionary: {e}",
  "the comment being answered is not in this document any more: {e}":
    "Der Kommentar, auf den du antwortest, ist nicht mehr in diesem Dokument: {e}",
  "that comment is not an annotation": "Dieser Kommentar ist keine Annotation",
  "Invalid signature or picture raster in save plan":
    "Ungültiges Unterschrifts- oder Bildraster im Speicherplan",
  "a mark on page {page} covers no area in that page's own space":
    "Eine Markierung auf Seite {page} bedeckt im Koordinatenraum dieser Seite keine Fläche",
  "a form field in the save plan names no kind of field":
    "Ein Formularfeld im Speicherplan nennt keine Feldart",
  "a link on page {page}: {why}": "Ein Link auf Seite {page}: {why}",
  "the comment being edited is not in this document any more: {e}":
    "Der Kommentar, den du bearbeitest, ist nicht mehr in diesem Dokument: {e}",
  "the comment being deleted is not in this document any more: {e}":
    "Der Kommentar, den du löschst, ist nicht mehr in diesem Dokument: {e}",
  "it goes to page {number}, and the document has {pages}": (a) =>
    `Er führt zu Seite ${a.number}, und das Dokument hat ${a.pages} ${n(a.pages, "Seite", "Seiten")}`,
  "it goes nowhere: write a page number or a web address in its box":
    "Er führt nirgendwohin: Schreib eine Seitenzahl oder eine Webadresse in sein Feld",
  "a link goes to a page number, or to a web address starting with http://, https:// or mailto:":
    "Ein Link führt zu einer Seitenzahl oder zu einer Webadresse, die mit http://, https:// oder mailto: beginnt",
  "this page's /Annots is not an array: {e}":
    "/Annots dieser Seite ist kein Array: {e}",

  // save/signed.rs
  "the file on disk is not the one that was opened, so it was not signed --- reopen it to sign what is there now":
    "Die Datei auf dem Datenträger ist nicht die, die geöffnet wurde, daher wurde nicht signiert - öffne sie neu, um zu signieren, was jetzt dort ist",
  "the signed document is written as a new file --- choose a name other than the original's":
    "Das signierte Dokument wird als neue Datei geschrieben - wähle einen anderen Namen als den des Originals",
  "could not write the signed document: {e}":
    "Das signierte Dokument konnte nicht geschrieben werden: {e}",

  // save_outside.rs: what the worker answered.
  "the worker checking the saved file did not answer within {seconds:.0} s, so the save could not be confirmed":
    "Der Arbeitsprozess, der die gespeicherte Datei prüft, hat nicht innerhalb von {seconds} s geantwortet, daher konnte das Speichern nicht bestätigt werden",
  "the worker could not take the document's password: {error}":
    "Der Arbeitsprozess konnte das Passwort des Dokuments nicht übernehmen: {error}",
  "the worker answered the re-read with {reply}":
    "Der Arbeitsprozess hat auf das erneute Lesen mit {reply} geantwortet",
  "no payload at all": "überhaupt keinem Inhalt",
  "the worker answered the validation data with {reply}":
    "Der Arbeitsprozess hat auf die Validierungsdaten mit {reply} geantwortet",
  "the worker answered the document timestamp with {reply}":
    "Der Arbeitsprozess hat auf den Dokumentzeitstempel mit {reply} geantwortet",
  "the worker answered the survey of the signatures with {reply}":
    "Der Arbeitsprozess hat auf die Übersicht der Signaturen mit {reply} geantwortet",
  "the worker answered the signature check with {reply}":
    "Der Arbeitsprozess hat auf die Signaturprüfung mit {reply} geantwortet",
  "the worker answered the verification with {reply}":
    "Der Arbeitsprozess hat auf die Prüfung mit {reply} geantwortet",
  "the worker answered a password with {reply}":
    "Der Arbeitsprozess hat auf ein Passwort mit {reply} geantwortet",
  "the worker abandoned the render":
    "Der Arbeitsprozess hat das Rendern abgebrochen",
  "the worker holding this document is gone":
    "Der Arbeitsprozess, der dieses Dokument hielt, ist beendet",
  "the worker reading the document did not answer within {seconds:.0} s":
    "Der Arbeitsprozess, der das Dokument liest, hat nicht innerhalb von {seconds} s geantwortet",
  "worker returned {stated} tile bytes; expected {expected} within {capacity}":
    "Der Arbeitsprozess hat {stated} Kachel-Bytes geliefert; erwartet waren {expected} bei höchstens {capacity}",
  "the worker answered the print job's read-back with {reply}":
    "Der Arbeitsprozess hat auf das Zurücklesen des Druckauftrags mit {reply} geantwortet",
  "the worker reading the print job back did not answer within {seconds} s":
    "Der Arbeitsprozess, der den Druckauftrag zurückliest, hat nicht innerhalb von {seconds} s geantwortet",
  "the worker answered the properties request with {reply}":
    "Der Arbeitsprozess hat auf die Anfrage nach den Eigenschaften mit {reply} geantwortet",
  "the worker answered the image request with {reply}":
    "Der Arbeitsprozess hat auf die Bildanfrage mit {reply} geantwortet",
  "the worker answered the signature request with {reply}":
    "Der Arbeitsprozess hat auf die Signaturanfrage mit {reply} geantwortet",
  "the worker answered with no payload at all":
    "Der Arbeitsprozess hat ganz ohne Inhalt geantwortet",
  "Reopen the source before creating an image-only redaction":
    "Öffne die Quelle neu, bevor du eine Schwärzung als Bild erstellst",
  "The source is too large or has changed since it was opened":
    "Die Quelle ist zu groß oder hat sich seit dem Öffnen geändert",
  "The source changed. Reopen it and mark the regions again":
    "Die Quelle hat sich geändert. Öffne sie neu und markiere die Bereiche noch einmal",
  "the worker answered the pictures with {reply}":
    "Der Arbeitsprozess hat auf die Bilder mit {reply} geantwortet",
  "the worker answered the picture with something else":
    "Der Arbeitsprozess hat auf das Bild mit etwas anderem geantwortet",
  "the worker stated a picture too large to hold":
    "Der Arbeitsprozess hat ein Bild angegeben, das zu groß zum Halten ist",
  "the worker answered with a picture past its limits":
    "Der Arbeitsprozess hat mit einem Bild jenseits seiner Grenzen geantwortet",
  "the worker answered the rewrite with {reply}":
    "Der Arbeitsprozess hat auf das Neuschreiben mit {reply} geantwortet",
  "the worker answered the merge with {reply}":
    "Der Arbeitsprozess hat auf das Zusammenführen mit {reply} geantwortet",
  "the worker answered the attachment with {reply}":
    "Der Arbeitsprozess hat auf den Anhang mit {reply} geantwortet",
  "the worker answered the print job with {reply}":
    "Der Arbeitsprozess hat auf den Druckauftrag mit {reply} geantwortet",

  // save_order.rs
  "there is nothing to save --- this document has no unsaved changes":
    "Es gibt nichts zu speichern - dieses Dokument hat keine ungespeicherten Änderungen",

  // commands/redact.rs
  "The fill did not run: {e}": "Die Füllung wurde nicht ausgeführt: {e}",
  "Content removal finished, but the fill could not be saved: {why}":
    "Der Inhalt wurde entfernt, aber die Füllung konnte nicht gespeichert werden: {why}",
  "the redacted file could not be read back: {why}":
    "Die geschwärzte Datei konnte nicht zurückgelesen werden: {why}",
  "the redacted file could not be measured: {why}":
    "Die Größe der geschwärzten Datei konnte nicht ermittelt werden: {why}",
  "the redacted file is {len} bytes, which is more than fits here":
    "Die geschwärzte Datei hat {len} Bytes, mehr als hier hineinpasst",
  "nothing in this document is marked for removal":
    "In diesem Dokument ist nichts zum Entfernen markiert",
  "the form fields could not be read, so an answer under a marked area cannot be checked: {why}":
    "Die Formularfelder konnten nicht gelesen werden, daher lässt sich eine Antwort unter einem markierten Bereich nicht prüfen: {why}",
  "the removed areas could not be checked, so the file cannot be shown clean: {e}":
    "Die entfernten Bereiche konnten nicht geprüft werden, daher lässt sich nicht zeigen, dass die Datei sauber ist: {e}",
  "The image-only copy did not run: {e}":
    "Die Kopie nur aus Bildern wurde nicht erstellt: {e}",
  "the redaction did not run: {e}":
    "Die Schwärzung wurde nicht ausgeführt: {e}",
  "the verification did not run: {e}":
    "Die Prüfung wurde nicht ausgeführt: {e}",
  "the file was written but could not be read back to check it: {why}":
    "Die Datei wurde geschrieben, konnte zur Prüfung aber nicht zurückgelesen werden: {why}",
  "the redaction did not finish: {e}":
    "Die Schwärzung wurde nicht abgeschlossen: {e}",

  // commands/save.rs
  "the save did not run: {e}": "Das Speichern wurde nicht ausgeführt: {e}",
  "the save did not finish: {e}":
    "Das Speichern wurde nicht abgeschlossen: {e}",
  "the extract did not run: {e}": "Das Extrahieren wurde nicht ausgeführt: {e}",
  "the split did not run: {e}": "Das Aufteilen wurde nicht ausgeführt: {e}",
  "the merge did not run: {e}":
    "Das Zusammenführen wurde nicht ausgeführt: {e}",
  "the document was not made: {e}": "Das Dokument wurde nicht erstellt: {e}",

  // pagetree.rs, merge.rs, sweep.rs
  "the page tree holds {count} pages and the renderer sees {page_count}":
    "Der Seitenbaum enthält {count} Seiten, der Renderer sieht {page_count}",
  "pages {first} and {second} are the same page in this file, so they cannot be turned differently. Turn them the same way, or leave both as they are.":
    "Die Seiten {first} und {second} sind in dieser Datei dieselbe Seite und lassen sich daher nicht unterschiedlich drehen. Drehe beide gleich oder lass beide, wie sie sind.",
  "page {id:?}: the crop {want:?} is outside the page {media:?}":
    "Seite {id}: Der Zuschnitt {want} liegt außerhalb der Seite {media}",
  "no document catalog: {e}": "Kein Dokumentkatalog: {e}",
  "no page tree: {e}": "Kein Seitenbaum: {e}",
  "the page tree root is not a dictionary: {e}":
    "Die Wurzel des Seitenbaums ist kein Dictionary: {e}",
  "an object nests deeper than {limit} levels":
    "Ein Objekt ist tiefer als {limit} Ebenen verschachtelt",
  "it has no pages": "Es hat keine Seiten",
  "no pages were asked for": "Es wurden keine Seiten angefordert",
  "it has {count} pages, so there is no page {page}":
    "Es hat {count} Seiten, eine Seite {page} gibt es also nicht",
  "page {page} was asked for twice": "Seite {page} wurde zweimal angefordert",
  "object {object} cannot be renumbered past {by} without overflowing":
    "Objekt {object} lässt sich nicht um {by} weiter nummerieren, ohne überzulaufen",
  "the page tree root has no kids: {e}":
    "Die Wurzel des Seitenbaums hat keine /Kids: {e}",

  // docgraph.rs
  "the document's bytes could not be read":
    "Die Bytes des Dokuments konnten nicht gelesen werden",
} satisfies Record<string, Wording>;

/** The entries above that say "du", addressing the reader with "Sie" instead. */
export const sie = {
  "the text you marked is inside a form that this document draws {times} time(s). Removing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Der markierte Text liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Ihn zu entfernen würde jede dieser Stellen ändern, auch solche, die Sie nicht markiert haben, daher wurde nichts entfernt.",
  "the picture you marked is inside a form that this document draws {times} time(s). Removing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Das markierte Bild liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Es zu entfernen würde jede dieser Stellen ändern, auch solche, die Sie nicht markiert haben, daher wurde nichts entfernt.",
  "the drawing you marked is inside a form that this document draws {times} time(s). Changing it would change every one of them, including places you did not mark, so nothing was removed.":
    "Die markierte Zeichnung liegt in einem Form-XObject, das dieses Dokument {times}-mal zeichnet. Die Zeichnung zu ändern würde jede dieser Stellen ändern, auch solche, die Sie nicht markiert haben, daher wurde nichts entfernt.",
  "{words} is too short to be told apart from the rest of the file: the check reads the whole file, where one or two characters also occur in the page's own numbers and other text, so it cannot say whether the marked one was removed --- mark the whole word or number to get an answer":
    "{words} ist zu kurz, um es vom Rest der Datei zu unterscheiden: Die Prüfung liest die ganze Datei, in der ein oder zwei Zeichen auch in den Zahlen und im übrigen Text der Seite vorkommen, daher kann sie nicht sagen, ob das Markierte entfernt wurde - markieren Sie das ganze Wort oder die ganze Zahl, um eine Antwort zu bekommen",
  "save text edits before applying redactions":
    "Speichern Sie die Textänderungen, bevor Sie Schwärzungen anwenden",
  "An image-only copy is drawn from this document's own pages, and some of these came from another document. Redact without the image-only option, or save this document, reopen it, and make an image-only copy there.":
    "Eine Kopie nur aus Bildern entsteht aus den eigenen Seiten dieses Dokuments, und einige davon stammen aus einem anderen Dokument. Schwärzen Sie ohne die Option „Kopie nur aus Bildern“ oder speichern Sie dieses Dokument, öffnen Sie es neu und erstellen Sie die Kopie nur aus Bildern dort.",
  "{reason} A print job is built from the file on disk, so the paper would carry that document with your edits placed on the pages of the one you opened.":
    "{reason} Ein Druckauftrag entsteht aus der Datei auf dem Datenträger, das Papier zeigte also jenes Dokument, mit Ihren Änderungen auf den Seiten des Dokuments, das Sie geöffnet haben.",
  "tpdf cannot save over the document it is reading --- choose another name":
    "tpdf kann nicht über das Dokument speichern, das es gerade liest - wählen Sie einen anderen Namen",
  "Choose a different name for the image-only copy; the original must be kept":
    "Wählen Sie einen anderen Namen für die Kopie nur aus Bildern; das Original muss erhalten bleiben",
  "The source could not be fingerprinted. Reopen it before redacting":
    "Von der Quelle ließ sich kein Fingerabdruck bilden. Öffnen Sie sie neu, bevor Sie schwärzen",
  "The source changed since it was opened. Reopen it and mark the regions again":
    "Die Quelle hat sich seit dem Öffnen geändert. Öffnen Sie sie neu und markieren Sie die Bereiche noch einmal",
  "{path} already exists, and a split would replace it --- choose another name":
    "{path} gibt es schon, und das Aufteilen würde es ersetzen - wählen Sie einen anderen Namen",
  "choose at least one document to merge in":
    "Wählen Sie mindestens ein Dokument zum Zusammenführen",
  "choose at least one picture":
    "Wählen Sie mindestens ein Bild",
  "the pictures come to {total} MB, and tpdf takes at most {limit} MB at once --- make the document in two passes and merge them":
    "Die Bilder sind zusammen {total} MB groß, und tpdf nimmt höchstens {limit} MB auf einmal - erstellen Sie das Dokument in zwei Durchgängen und führen Sie sie zusammen",
  "the documents to merge come to at least {total} MB, and tpdf merges at most {limit} MB at once --- merge fewer files, or merge them in two passes":
    "Die zusammenzuführenden Dokumente sind zusammen mindestens {total} MB groß, und tpdf führt höchstens {limit} MB auf einmal zusammen - führen Sie weniger Dateien zusammen oder arbeiten Sie in zwei Durchgängen",
  "tpdf could not record what this file looked like when it was opened, so it cannot tell whether saving over it is safe --- use Save a copy":
    "tpdf konnte nicht festhalten, wie diese Datei beim Öffnen aussah, und kann daher nicht sagen, ob Speichern darüber sicher ist - nutzen Sie „Kopie speichern“",
  "tpdf could not confirm what this file was when it read it --- use Save a copy":
    "tpdf konnte nicht bestätigen, welche Datei es beim Lesen vor sich hatte - nutzen Sie „Kopie speichern“",
  "tpdf did not record what {label} was when its pages were inserted, so it cannot tell whether they are still those pages --- undo the insert and insert them again":
    "tpdf hat nicht festgehalten, wie {label} beim Einfügen seiner Seiten aussah, und kann daher nicht sagen, ob es noch dieselben Seiten sind - machen Sie das Einfügen rückgängig und fügen Sie sie noch einmal ein",
  "{label} has changed since its pages were inserted, so they may no longer be the pages you placed --- undo the insert and insert them again":
    "{label} hat sich seit dem Einfügen seiner Seiten geändert, es sind also vielleicht nicht mehr die Seiten, die Sie eingefügt haben - machen Sie das Einfügen rückgängig und fügen Sie sie noch einmal ein",
  "these edits were built against {built} bytes and the file is {size} --- reopen it before saving":
    "Diese Änderungen beziehen sich auf {built} Bytes, die Datei hat {size} - öffnen Sie sie vor dem Speichern neu",
  "This document is encrypted and tpdf could not unlock it, so it cannot be saved. Open it with its password first.":
    "Dieses Dokument ist verschlüsselt und tpdf konnte es nicht entsperren, daher kann es nicht gespeichert werden. Öffnen Sie es zuerst mit seinem Passwort.",
  "the document on disk has {pages} page(s) and the edits were made against {baseline} --- it has changed since it was opened, so reopen it before saving":
    "Das Dokument auf dem Datenträger hat {pages} Seite(n), die Änderungen beziehen sich auf {baseline} - es hat sich seit dem Öffnen geändert, öffnen Sie es also vor dem Speichern neu",
  "{why} --- nothing was written, so the file on disk is untouched. Your document has been closed and the edits you had made are gone.":
    "{why} - es wurde nichts geschrieben, die Datei auf dem Datenträger ist also unverändert. Ihr Dokument wurde geschlossen, und Ihre Änderungen sind verloren.",
  "tpdf could not tell which file {source:?} is, so it cannot promise a save would land there --- nothing was written. Use Save a copy.":
    "tpdf konnte nicht feststellen, welche Datei {source} ist, und kann daher nicht zusichern, dass ein Speichern dort ankommt - es wurde nichts geschrieben. Nutzen Sie „Kopie speichern“.",
  "save this document before merging it --- it holds pages inserted from another document, and a merge cannot carry those yet":
    "Speichern Sie dieses Dokument, bevor Sie es zusammenführen - es enthält aus einem anderen Dokument eingefügte Seiten, und ein Zusammenführen kann die noch nicht mitnehmen",
  "{label} is encrypted, and merging rewrites it --- which would silently remove that. Leave it out, or save an unencrypted copy of it first.":
    "{label} ist verschlüsselt, und das Zusammenführen schreibt es neu - dabei ginge die Verschlüsselung stillschweigend verloren. Lassen Sie es weg oder speichern Sie zuerst eine unverschlüsselte Kopie davon.",
  "This document is encrypted and tpdf could not unlock it, so it cannot be rewritten. Open it with its password first.":
    "Dieses Dokument ist verschlüsselt und tpdf konnte es nicht entsperren, daher kann es nicht neu geschrieben werden. Öffnen Sie es zuerst mit seinem Passwort.",
  "{label} is encrypted, and inserting its pages would write them without that --- save an unencrypted copy of it and insert from that instead":
    "{label} ist verschlüsselt, und beim Einfügen seiner Seiten würden sie ohne Verschlüsselung geschrieben - speichern Sie eine unverschlüsselte Kopie davon und fügen Sie stattdessen aus dieser ein",
  "pages {gone} and {shared} are the same page in this file, so page {again} cannot be removed on its own. Remove both, or keep both.":
    "Die Seiten {gone} und {shared} sind in dieser Datei dieselbe Seite, daher kann Seite {again} nicht allein entfernt werden. Entfernen Sie beide oder behalten Sie beide.",
  "pages {first} and {second} are the same page in this file, so they cannot be cropped differently. Crop them the same way, or leave both as they are.":
    "Die Seiten {first} und {second} sind in dieser Datei dieselbe Seite und lassen sich daher nicht unterschiedlich zuschneiden. Schneiden Sie beide gleich zu oder lassen Sie beide, wie sie sind.",
  "the comment being answered is not in this document any more: {e}":
    "Der Kommentar, auf den Sie antworten, ist nicht mehr in diesem Dokument: {e}",
  "the comment being edited is not in this document any more: {e}":
    "Der Kommentar, den Sie bearbeiten, ist nicht mehr in diesem Dokument: {e}",
  "the comment being deleted is not in this document any more: {e}":
    "Der Kommentar, den Sie löschen, ist nicht mehr in diesem Dokument: {e}",
  "the file on disk is not the one that was opened, so it was not signed --- reopen it to sign what is there now":
    "Die Datei auf dem Datenträger ist nicht die, die geöffnet wurde, daher wurde nicht signiert - öffnen Sie sie neu, um zu signieren, was jetzt dort ist",
  "the signed document is written as a new file --- choose a name other than the original's":
    "Das signierte Dokument wird als neue Datei geschrieben - wählen Sie einen anderen Namen als den des Originals",
  "Reopen the source before creating an image-only redaction":
    "Öffnen Sie die Quelle neu, bevor Sie eine Schwärzung als Bild erstellen",
  "The source changed. Reopen it and mark the regions again":
    "Die Quelle hat sich geändert. Öffnen Sie sie neu und markieren Sie die Bereiche noch einmal",
  "pages {first} and {second} are the same page in this file, so they cannot be turned differently. Turn them the same way, or leave both as they are.":
    "Die Seiten {first} und {second} sind in dieser Datei dieselbe Seite und lassen sich daher nicht unterschiedlich drehen. Drehen Sie beide gleich oder lassen Sie beide, wie sie sind.",
} satisfies Record<string, Wording>;
