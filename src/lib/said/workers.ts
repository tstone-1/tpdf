/** Workers, rendering, printing and text recognition: `workers.rs`,
 * `worker_child.rs`, `worker.rs`, `worker_shm.rs`, `render.rs`,
 * `sandbox_win.rs`, `print.rs`, `print_win.rs`, `print_linux.rs`,
 * `print_macos.rs`, `print_layout.rs`, `printers_win.rs`, `commands/print.rs`,
 * `ocr.rs`, `ocr_worker.rs`, `ocr_tesseract.rs`, `ocr_gate.rs`,
 * `ocr_windows.rs`, `ocr_vision.rs` and `commands/ocr.rs`. */

import type { Wording } from "./de";

/** `n Seite` or `n Seiten`. */
function pages(n: string): string {
  return n === "1" ? "1 Seite" : `${n} Seiten`;
}

/** `ocr_layer::pages_named`'s `page 2` / `pages 2, 5 and 3 more`, in German. */
function pagesNamed(english: string): string {
  return english
    .replace(/^pages /, "Seiten ")
    .replace(/^page /, "Seite ")
    .replace(/ and (\d+) more$/, " und $1 weitere");
}

/** `ocr::Script` names, joined by `listed` with " and ", in German. */
const SCRIPTS: Record<string, string> = {
  Latin: "Lateinisch",
  Greek: "Griechisch",
  Cyrillic: "Kyrillisch",
  Hebrew: "Hebräisch",
  Arabic: "Arabisch",
  Devanagari: "Devanagari",
  Thai: "Thai",
  Korean: "Koreanisch",
  hiragana: "Hiragana",
  katakana: "Katakana",
  "Chinese characters": "chinesische Schriftzeichen",
  "digits and punctuation": "Ziffern und Satzzeichen",
};

function scripts(english: string): string {
  if (english in SCRIPTS) return SCRIPTS[english]!;
  return english
    .split(" and ")
    .map((name) => SCRIPTS[name] ?? name.replace(/^the script at /, "die Schrift bei "))
    .join(" und ");
}

export default {
  // workers.rs
  "{OUTLIVED_MARK}: it was {mapped} bytes when it was opened and is {backing} now, so the part of the document past that point is gone. Open it again to read the new file.":
    (a) =>
      `Diese Datei wurde auf dem Datenträger geändert, während sie geöffnet war: Beim Öffnen hatte sie ${a.mapped} Bytes, jetzt hat sie ${a.backing}, daher fehlt der Teil des Dokuments ab dieser Stelle. Öffne sie erneut, um die neue Datei zu lesen.`,
  "a replacement worker could not unlock the document: {e}":
    "ein Ersatz-Hintergrundprozess konnte das Dokument nicht entsperren: {e}",
  "{error} --- the request exceeded its deadline": "{error} – die Anfrage hat ihre Frist überschritten",
  "{path:?} is empty": "{path} ist leer",
  "{e} --- the document did not open within the deadline":
    "{e} – das Dokument wurde nicht innerhalb der Frist geöffnet",

  // worker_child.rs
  "tpdf could not load its PDF engine. Reinstalling should fix it. ({e})":
    "tpdf konnte seine PDF-Engine nicht laden. Eine Neuinstallation sollte das beheben. ({e})",
  "That password did not open this document.": "Mit diesem Passwort lässt sich das Dokument nicht öffnen.",

  // worker.rs and sandbox_win.rs: what became of a worker
  "still running": "läuft noch",
  "killed by signal {signal}": "durch Signal {signal} beendet",
  "exited with code {code}": "mit Code {code} beendet",
  "exited with {how}": "beendet mit {how}",
  "killed by its parent": "vom Hauptprozess beendet",
  "could not be waited on: {e}": "auf das Ende konnte nicht gewartet werden: {e}",
  "spawn failed: {e}": "der Hintergrundprozess konnte nicht gestartet werden: {e}",
  "worker stopped answering ({why})": "der Hintergrundprozess antwortet nicht mehr ({why})",

  // worker_shm.rs
  "could not open {path:?}: {e}": "{path} konnte nicht geöffnet werden: {e}",
  "could not stat {path:?}: {e}": "die Angaben zu {path} konnten nicht gelesen werden: {e}",
  "could not map {path:?}: {e}": "{path} konnte nicht in den Speicher eingeblendet werden: {e}",

  // render.rs
  "document {doc} has been closed": "Dokument {doc} wurde geschlossen",
  "no such document: {doc}": "dieses Dokument gibt es nicht: {doc}",
  "the render service has stopped": "die Darstellung wurde beendet",
  "render failed with Pdfium status {status}": "die Darstellung ist mit PDFium-Status {status} fehlgeschlagen",
  "text preview was cancelled": "die Textvorschau wurde abgebrochen",
  "the preview could not be drawn: {reason}": "die Vorschau konnte nicht gezeichnet werden: {reason}",
  "the preview was cancelled": "die Vorschau wurde abgebrochen",
  "the drawing was abandoned": "das Zeichnen wurde abgebrochen",

  // print.rs
  "This document is encrypted, and printing a selection of it would produce an unencrypted copy. Print the whole document instead --- that is handed over unchanged.":
    "Dieses Dokument ist verschlüsselt, und eine Auswahl daraus zu drucken würde eine unverschlüsselte Kopie erzeugen. Drucke stattdessen das ganze Dokument – das wird unverändert übergeben.",
  "the print job has {found} pages, not the {expected} asked for": (a) =>
    `der Druckauftrag hat ${pages(a.found!)}, nicht die angeforderten ${a.expected}`,
  "the print job came out empty": "der Druckauftrag ist leer geworden",
  "pages {first} to {last} run backwards": "die Seiten {first} bis {last} laufen rückwärts",
  "there is no page 0 to print from": "es gibt keine Seite 0, ab der gedruckt werden könnte",
  "page {last} is not in this job, which has {count}": (a) =>
    `Seite ${a.last} ist nicht in diesem Druckauftrag, der ${pages(a.count!)} hat`,
  "No page has reached the printer": "Keine Seite hat den Drucker erreicht",
  "1 page reached the printer, and none since": "1 Seite hat den Drucker erreicht, seitdem keine mehr",
  "{many} pages reached the printer, and none since": "{many} Seiten haben den Drucker erreicht, seitdem keine mehr",
  "{sent} for a minute. The print queue may be stuck: open Settings, Printers & scanners, choose the printer and open its print queue. If the queue starts again, the rest will print.":
    "{sent} (seit einer Minute). Die Druckwarteschlange hängt vielleicht: Öffne Einstellungen, Drucker und Scanner, wähle den Drucker und öffne seine Druckwarteschlange. Läuft die Warteschlange wieder an, wird der Rest gedruckt.",
  "this document has a page tpdf made, which a page range cannot name":
    "dieses Dokument hat eine Seite, die tpdf erstellt hat und die kein Seitenbereich benennen kann",
  "no pages selected": "keine Seiten ausgewählt",
  "page {page} is not in this document, which has {count}": (a) =>
    `Seite ${a.page} ist nicht in diesem Dokument, das ${pages(a.count!)} hat`,

  // print_win.rs
  "the OS parser could not read the print job": "der PDF-Leser des Betriebssystems konnte den Druckauftrag nicht lesen",
  "a page {width} pixels wide is too wide to print": "eine Seite mit {width} Pixeln Breite ist zu breit zum Drucken",
  "a {width}x{height} page is too large to print": "eine Seite mit {width}x{height} ist zu groß zum Drucken",
  "page too wide": "Seite zu breit",
  "page too tall": "Seite zu hoch",
  "band too tall": "Streifen zu hoch",
  "the worker could not open the print job: {why}":
    "der Hintergrundprozess konnte den Druckauftrag nicht öffnen: {why}",
  "the worker returned incomplete page sizes": "der Hintergrundprozess hat unvollständige Seitengrößen geliefert",
  "sheet {index} is not in this job, which has {count}": "Blatt {index} ist nicht in diesem Druckauftrag, der {count} hat",
  "this print job has no pages in it": "dieser Druckauftrag enthält keine Seiten",
  "the print dialog failed: CDERR {code:#06x}": "der Druckdialog ist fehlgeschlagen: CDERR {code}",

  // print_linux.rs
  "GTK would not make a print dialog": "GTK hat keinen Druckdialog erstellt",
  "no printer was chosen": "es wurde kein Drucker gewählt",
  "the printer {name:?} cannot take a PDF, and tpdf prints only the PDF itself":
    "der Drucker {name} nimmt kein PDF an, und tpdf druckt nur das PDF selbst",
  "GTK would not make a print job": "GTK hat keinen Druckauftrag erstellt",
  "GTK would not take the print job: {why}": "GTK hat den Druckauftrag nicht angenommen: {why}",
  "{name:?} prints every page of the document, so it cannot print only the pages chosen; choose All Pages, or a printer":
    "{name} druckt jede Seite des Dokuments und kann deshalb nicht nur die gewählten Seiten drucken; wähle „Alle Seiten“ oder einen Drucker",
  "GTK dropped the print job without an answer": "GTK hat den Druckauftrag ohne Antwort fallen gelassen",
  "GTK did not say the job was sent within {secs} s":
    "GTK hat nicht innerhalb von {secs} s gemeldet, dass der Auftrag gesendet wurde",
  "no printer named {name:?}; GTK listed {seen:?}": "kein Drucker mit dem Namen {name}; GTK hat {seen} aufgeführt",
  "no reason was given": "es wurde kein Grund genannt",

  // print_macos.rs
  "PDFKit could not read the print job": "PDFKit konnte den Druckauftrag nicht lesen",
  "PDFKit would not build a print operation for this document":
    "PDFKit hat für dieses Dokument keinen Druckvorgang erstellt",

  // print_layout.rs
  "{other} pages to a sheet is not a choice": "{other} Seiten pro Blatt stehen nicht zur Auswahl",
  "the paper has no printable area": "das Papier hat keinen bedruckbaren Bereich",
  "a scale of {percent}% is not between 1% and {MOST_PERCENT}%":
    "eine Skalierung von {percent} % liegt nicht zwischen 1 % und {MOST_PERCENT} %",
  "a poster takes one page to a sheet": "ein Poster braucht eine Seite pro Blatt",
  "page {page} as a poster at {percent}% takes {count} sheets, more than the {MOST_POSTER_SHEETS} tpdf prints of one page":
    "Seite {page} braucht als Poster mit {percent} % {count} Blätter, mehr als die {MOST_POSTER_SHEETS}, die tpdf von einer Seite druckt",
  "page {page} is not in this job": "Seite {page} ist nicht in diesem Druckauftrag",
  "page {page} has no size": "Seite {page} hat keine Größe",

  // printers_win.rs
  "the printers could not be listed: {e}": "die Drucker konnten nicht aufgelistet werden: {e}",
  "the printer {name} could not be opened: {e}": "der Drucker {name} konnte nicht geöffnet werden: {e}",
  "the driver of {name} gives no settings": "der Treiber von {name} liefert keine Einstellungen",
  "the driver of {name} gives no settings ({got})": "der Treiber von {name} liefert keine Einstellungen ({got})",
  "the driver of {name} refused the settings ({other})": "der Treiber von {name} hat die Einstellungen abgelehnt ({other})",
  "the printer {name} could not be prepared: {e}": "der Drucker {name} konnte nicht vorbereitet werden: {e}",

  // commands/print.rs
  "the print panel could not be shown: {e}": "der Druckdialog konnte nicht angezeigt werden: {e}",
  "the print job could not be built: {e}": "der Druckauftrag konnte nicht erstellt werden: {e}",
  "the print preview is unavailable": "die Druckvorschau ist nicht verfügbar",
  "another page of this preview is being drawn": "eine andere Seite dieser Vorschau wird gerade gezeichnet",
  "the print preview could not start: {e}": "die Druckvorschau konnte nicht starten: {e}",
  "the printer is chosen in the system's print dialog here":
    "der Drucker wird hier im Druckdialog des Systems gewählt",
  "the printer could not be asked: {e}": "der Drucker konnte nicht abgefragt werden: {e}",
  "no printer is installed": "es ist kein Drucker installiert",
  "no page is chosen to print": "es ist keine Seite zum Drucken gewählt",
  "the print job could not be read: {why}": "der Druckauftrag konnte nicht gelesen werden: {why}",
  "the worker returned incomplete page sizes for the print job":
    "der Hintergrundprozess hat für den Druckauftrag unvollständige Seitengrößen geliefert",
  "the page could not be drawn: {e}": "die Seite konnte nicht gezeichnet werden: {e}",
  "that page is not in the print job": "diese Seite ist nicht im Druckauftrag",
  "the page is too wide": "die Seite ist zu breit",
  "the page is too tall": "die Seite ist zu hoch",
  "a tile is too wide": "eine Kachel ist zu breit",
  "a tile is too tall": "eine Kachel ist zu hoch",
  "the page has no size to draw": "die Seite hat keine Größe, in der sie gezeichnet werden könnte",
  "the page is too tall to preview": "die Seite ist zu hoch für die Vorschau",
  "no printer has been chosen for this print": "für diesen Druck wurde kein Drucker gewählt",
  "the print could not start: {e}": "der Druck konnte nicht starten: {e}",
  "the print job could not be read back, so it will not be printed":
    "der Druckauftrag konnte nicht zurückgelesen werden und wird deshalb nicht gedruckt",
  "the working document has no plan to print": "das Arbeitsdokument hat keinen Plan zum Drucken",
  "the print panel could not be shown on the main thread: {e}":
    "der Druckdialog konnte im Hauptthread nicht angezeigt werden: {e}",
  "the print job could not be read back, so it will not be printed: {e}":
    "der Druckauftrag konnte nicht zurückgelesen werden und wird deshalb nicht gedruckt: {e}",

  // ocr.rs
  "no engine available: {detail}": "keine Texterkennung verfügbar: {detail}",
  "the engine crashed: {detail}": "die Texterkennung ist abgestürzt: {detail}",
  "the engine timed out: {detail}": "die Texterkennung hat die Zeit überschritten: {detail}",
  "the engine rejected the image: {detail}": "die Texterkennung hat das Bild abgelehnt: {detail}",
  "the image buffer is malformed: {detail}": "der Bildpuffer ist fehlerhaft: {detail}",
  "the selected area has no extracted word boxes with a usable height (for example, its visible words may be images), leaving nothing to size a control against; OCR verification cannot certify this removal":
    "der ausgewählte Bereich hat keine erkannten Wortrahmen mit brauchbarer Höhe (seine sichtbaren Wörter sind zum Beispiel vielleicht Bilder), daher gibt es nichts, woran sich die Größe eines Kontrollworts ausrichten ließe; die Prüfung per Texterkennung kann diese Entfernung nicht bestätigen",
  "an empty control token is read back by every engine and by none":
    "ein leeres Kontrollwort wird von jeder Texterkennung gelesen und von keiner",
  "the regions cover every word on this page, so there is nothing left for the engine to read back and nothing here can be certified":
    "die Bereiche decken jedes Wort auf dieser Seite ab, daher bleibt nichts, was die Texterkennung zurücklesen könnte, und hier lässt sich nichts bestätigen",
  "every word left on this page is set larger than the {size_pt:.1} pt that was removed, so reading one back would say nothing about the small print":
    "jedes verbliebene Wort auf dieser Seite ist größer gesetzt als die entfernten {size_pt} pt, daher würde das Zurücklesen eines davon nichts über das Kleingedruckte aussagen",
  "no word left on this page draws {MIN_CONTROL_CHARS} characters at {size_pt:.1} pt or smaller, and a shorter control is read back by accident":
    "kein verbliebenes Wort auf dieser Seite hat {MIN_CONTROL_CHARS} Zeichen in {size_pt} pt oder kleiner, und ein kürzeres Kontrollwort wird zufällig gelesen",
  "the control token {token:?}, drawn at {size:.1} pt, was not read back from the probe image. The engine is not able to read text of that size here, so its finding nothing else says nothing about what survived.":
    "das Kontrollwort {token}, in {size} pt gezeichnet, wurde im Prüfbild nicht gelesen. Die Texterkennung kann Text dieser Größe hier nicht lesen, daher sagt es nichts darüber aus, was übrig geblieben ist, wenn sie sonst nichts findet.",
  "the text removed here was written in {removed}, and the control word {token:?} only shows that the engine reads {shown} on this page. Its finding nothing in a script it was not shown to read says nothing about what survived.":
    (a) =>
      `der hier entfernte Text ist in folgender Schrift geschrieben: ${scripts(a.removed!)}, und das Kontrollwort ${a.token} zeigt nur, dass die Texterkennung auf dieser Seite Folgendes liest: ${scripts(a.shown!)}. Dass sie in einer Schrift, deren Lesen nicht gezeigt wurde, nichts findet, sagt nichts darüber aus, was übrig geblieben ist.`,
  "{count} legible span(s) survive; these pixels are not redacted": (a) =>
    a.count === "1"
      ? "1 lesbarer Abschnitt ist übrig; diese Pixel sind nicht geschwärzt"
      : `${a.count} lesbare Abschnitte sind übrig; diese Pixel sind nicht geschwärzt`,
  "not verified, so not certified: {why}": "nicht geprüft, daher nicht bestätigt: {why}",

  // ocr_worker.rs
  "the OCR worker stopped listening: {e}": "der Texterkennungsprozess nimmt keine Anfragen mehr an: {e}",
  "the OCR worker's reply broke: {e}": "die Antwort des Texterkennungsprozesses ist abgebrochen: {e}",
  "the OCR worker stopped without answering": "der Texterkennungsprozess wurde ohne Antwort beendet",
  "the OCR worker did not answer within {secs} seconds":
    "der Texterkennungsprozess hat nicht innerhalb von {secs} Sekunden geantwortet",
  "the OCR worker's reply did not parse: {e}": "die Antwort des Texterkennungsprozesses war nicht lesbar: {e}",

  // ocr_tesseract.rs
  "no text-recognition language data is installed": "es sind keine Sprachdaten für die Texterkennung installiert",
  "tpdf has no text-recognition data for {asked}; it reads {offered}":
    "tpdf hat keine Texterkennungsdaten für {asked}; es liest {offered}",
  "text recognition needs Tesseract 5, and {library} could not be loaded ({e}); on Ubuntu and Debian it is the package libtesseract5, on Fedora tesseract-libs":
    "die Texterkennung braucht Tesseract 5, und {library} konnte nicht geladen werden ({e}); unter Ubuntu und Debian ist das das Paket libtesseract5, unter Fedora tesseract-libs",
  "tpdf's text-recognition language data is not where tpdf looks for it; reinstalling should fix it":
    "die Sprachdaten von tpdf für die Texterkennung liegen nicht dort, wo tpdf sie sucht; eine Neuinstallation sollte das beheben",
  "Tesseract could not start with {codes} from {dir}": "Tesseract konnte nicht mit {codes} aus {dir} starten",
  "Tesseract could not read the image": "Tesseract konnte das Bild nicht lesen",
  "Tesseract recognised the image and returned no results": "Tesseract hat das Bild erkannt und keine Ergebnisse geliefert",

  // ocr_gate.rs
  "the probe image would be {width_pt} x {probe_height_pt} pt and the control is {size_pt} pt, so no render scale can be chosen":
    "das Prüfbild wäre {width_pt} x {probe_height_pt} pt groß und das Kontrollwort {size_pt} pt, daher lässt sich kein Darstellungsmaßstab wählen",
  "the probe image for this region is {width_pt:.0} x {probe_height_pt:.0} pt, which at the smallest scale the engine can read at is {at_floor} bytes against a {capacity} byte buffer, so it could not be rendered for the check":
    "das Prüfbild für diesen Bereich ist {width_pt} x {probe_height_pt} pt groß, was beim kleinsten Maßstab, den die Texterkennung lesen kann, {at_floor} Bytes bei einem Puffer von {capacity} Bytes ergibt; daher konnte es für die Prüfung nicht dargestellt werden",
  "the smallest thing this page removed is {control_pt:.1} pt, and a control that size needs {wanted:.1}x to reach {MIN_CONTROL_PX:.0} px — past the {MAX_SCALE:.0}x ceiling, so no rendering of this page can prove the removal":
    "das Kleinste, was auf dieser Seite entfernt wurde, ist {control_pt} pt groß, und ein Kontrollwort dieser Größe braucht {wanted}x, um {MIN_CONTROL_PX} px zu erreichen — mehr als die Obergrenze von {MAX_SCALE}x, daher kann keine Darstellung dieser Seite die Entfernung belegen",
  "the area at {rect:?} pt is not on a page {width_px} px wide, so there was nothing to read for the check":
    "der Bereich bei {rect} pt liegt nicht auf einer {width_px} px breiten Seite, daher gab es für die Prüfung nichts zu lesen",
  "page {page}: the removed area still reads as text when the written page is rendered — {read}":
    "Seite {page}: der entfernte Bereich ist noch als Text lesbar, wenn die geschriebene Seite dargestellt wird — {read}",
  "page {page}: the removed area could not be shown unreadable. {why}":
    "Seite {page}: es ließ sich nicht zeigen, dass der entfernte Bereich unlesbar ist. {why}",
  "something the engine could not quote": "etwas, das die Texterkennung nicht wiedergeben konnte",
  "page {page}: the marked areas held no text, so the check was sized from the smallest print left on the page: nothing {size:.1} pt or larger is readable there":
    "Seite {page}: die markierten Bereiche enthielten keinen Text, daher richtet sich die Prüfung nach der kleinsten Schrift, die auf der Seite übrig ist: dort ist nichts in {size} pt oder größer lesbar",
  "the removed areas could not be read back. {why}": "die entfernten Bereiche konnten nicht zurückgelesen werden. {why}",
  "the written file could not be reopened to read the removed areas back, so they could not be shown unreadable: {reason}":
    "die geschriebene Datei konnte nicht erneut geöffnet werden, um die entfernten Bereiche zurückzulesen, daher ließ sich nicht zeigen, dass sie unlesbar sind: {reason}",
  "the area at {rect:?} pt is not on a page of {height_px} px, so there was nothing to render for the check":
    "der Bereich bei {rect} pt liegt nicht auf einer Seite mit {height_px} px, daher gab es für die Prüfung nichts darzustellen",
  "this page is {width_px} px wide at scale {scale}, too wide to render as one strip":
    "diese Seite ist beim Maßstab {scale} {width_px} px breit, zu breit, um sie als einen Streifen darzustellen",
  "this area is {rows} px tall at scale {scale}, too tall to render as one strip":
    "dieser Bereich ist beim Maßstab {scale} {rows} px hoch, zu hoch, um ihn als einen Streifen darzustellen",
  "the render of this area was abandoned, so it could not be checked":
    "die Darstellung dieses Bereichs wurde abgebrochen, daher konnte er nicht geprüft werden",
  "the render service did not answer within {secs} s while checking the removed areas":
    "die Darstellung hat beim Prüfen der entfernten Bereiche nicht innerhalb von {secs} s geantwortet",

  // ocr_windows.rs and ocr_vision.rs
  "listing languages: {e}": "beim Auflisten der Sprachen: {e}",
  "no engine for {tag}: {e}": "keine Texterkennung für {tag}: {e}",
  "no OCR recogniser language pack is installed on this machine":
    "auf diesem Rechner ist kein Sprachpaket für die Texterkennung installiert",
  "no engine from the profile ({profile}) and none from {tag} ({e})":
    "keine Texterkennung aus dem Profil ({profile}) und keine aus {tag} ({e})",
  "{width}x{height} exceeds this engine's largest edge of {max} px":
    "{width}x{height} überschreitet die größte Kantenlänge dieser Texterkennung von {max} px",
  "Vision refused the image when asked to read any script: {e}":
    "Vision hat das Bild abgelehnt, als es jede Schrift lesen sollte: {e}",
  "Vision refused the image: {e}": "Vision hat das Bild abgelehnt: {e}",

  // commands/ocr.rs
  "Text recognition stopped: {e}": "Die Texterkennung wurde beendet: {e}",
  "This page is too large to recognise.": "Diese Seite ist zu groß für die Texterkennung.",
  "The page render was abandoned.": "Die Darstellung der Seite wurde abgebrochen.",
  "The page was rendered at the wrong size.": "Die Seite wurde in der falschen Größe dargestellt.",
  "The selected recognition language is unavailable. Choose another language.":
    "Die gewählte Erkennungssprache ist nicht verfügbar. Wähle eine andere Sprache.",
  "No words could be recognised on this page.": "Auf dieser Seite konnten keine Wörter erkannt werden.",
  "Recognition returned invalid word positions.": "Die Texterkennung hat ungültige Wortpositionen geliefert.",
  "the render of page {n} was abandoned": "die Darstellung von Seite {n} wurde abgebrochen",
  "page {n} was rendered at the wrong size": "Seite {n} wurde in der falschen Größe dargestellt",
  "The text could not be read. {why}": "Der Text konnte nicht gelesen werden. {why}",
  "Page {n} could not be read: {why}": "Seite {n} konnte nicht gelesen werden: {why}",
  "No page was given text. The recogniser refused {pages}, {means}.": (a) =>
    `Keine Seite hat Text bekommen. Die Texterkennung hat ${pagesNamed(a.pages!)} abgelehnt; das bedeutet meist eine Schrift, die sie nicht lesen kann, oder einen Scan, der zu undeutlich ist, um die Schrift zu erkennen.`,
  "Every page already has text, so there is nothing to add.":
    "Jede Seite hat schon Text, daher gibt es nichts hinzuzufügen.",
  "No text was recognised on any page.": "Auf keiner Seite wurde Text erkannt.",
  "Every page is too large to render finely enough to read its text.":
    "Jede Seite ist zu groß, um sie fein genug darzustellen, dass ihr Text gelesen werden kann.",
  "No page was given text: each one either has text already, or none was recognised on it.":
    "Keine Seite hat Text bekommen: Jede hat entweder schon Text, oder auf ihr wurde keiner erkannt.",
  "The copy could not be opened to check it, so it was not kept: {reason}":
    "Die Kopie konnte zur Prüfung nicht geöffnet werden und wurde deshalb nicht behalten: {reason}",
  "The copy has {count} pages where the document has {pages}, so it was not kept.": (a) =>
    `Die Kopie hat ${pages(a.count!)}, das Dokument aber ${pages(a.pages!)}, daher wurde sie nicht behalten.`,
  "Page {page} of the copy did not read back with the text that was recognised, so the copy was not kept.":
    "Seite {page} der Kopie enthielt beim Zurücklesen nicht den erkannten Text, daher wurde die Kopie nicht behalten.",
  "{why} No copy was written.": "{why} Es wurde keine Kopie geschrieben.",
  "Text recognition did not run: {e}": "Die Texterkennung ist nicht gelaufen: {e}",
  "The languages could not be listed: {e}": "Die Sprachen konnten nicht aufgelistet werden: {e}",
} satisfies Record<string, Wording>;
