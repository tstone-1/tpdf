/** The application around the documents: opening and revealing files, the
 * session, the command-line tool, Explorer's handlers, the default PDF app,
 * the menu, passwords, smaller copies, SyncTeX, the file check before a save,
 * and the check for text the pages do not show (`hidden/survey.rs`). */

import type { Wording } from "./de";

/** "3 pages" or "1 page" in German, with a leading "; " left where it is. */
const seiten = (text: string): string =>
  text.replace(/(\d+) pages?\b/, (_, n: string) => (n === "1" ? "1 Seite" : `${n} Seiten`));

/** A count that may carry the "; " of the clause before it: is it one? */
const one = (count: string): boolean => count.replace(/^; /, "") === "1";

/** `compare.rs` names the two sides "first" and "second". */
const SIDE: Record<string, string> = { first: "erste", second: "zweite" };

export default {
  // commands/document.rs
  "The file no longer exists at this location": "Die Datei ist an diesem Ort nicht mehr vorhanden",
  "The document has no folder": "Das Dokument liegt in keinem Ordner",
  "Could not read the folder: {error}": "Der Ordner konnte nicht gelesen werden: {error}",
  "The file path is not valid Unicode": "Der Dateipfad ist kein gültiges Unicode",
  "The file path is invalid": "Der Dateipfad ist ungültig",
  "Could not show the file in Explorer: {error}": "Die Datei konnte im Explorer nicht angezeigt werden: {error}",
  "Could not show the file in Finder: {error}": "Die Datei konnte im Finder nicht angezeigt werden: {error}",
  "Finder could not show the file": "Der Finder konnte die Datei nicht anzeigen",
  "The file is in no folder": "Die Datei liegt in keinem Ordner",
  "No file manager answered, and the folder could not be opened: {error}":
    "Kein Dateimanager hat geantwortet, und der Ordner konnte nicht geöffnet werden: {error}",
  "The file path cannot be named as a URI": "Der Dateipfad lässt sich nicht als URI angeben",
  "a document of {count} pages is past what tpdf can edit":
    "Ein Dokument mit {count} Seiten übersteigt, was tpdf bearbeiten kann",
  "no document is open under {doc}": "Unter {doc} ist kein Dokument geöffnet",
  "Could not open {name}: {e}": "{name} konnte nicht geöffnet werden: {e}",
  "Could not insert pages from {name}: {why}": "Seiten aus {name} konnten nicht eingefügt werden: {why}",
  "Could not read {name}: {e}": "{name} konnte nicht gelesen werden: {e}",
  "the file check did not run: {e}": "Die Prüfung der Datei ist nicht gelaufen: {e}",

  // commands/read.rs
  "The recognised text exceeds the page limit.": "Der erkannte Text übersteigt die Grenze für eine Seite.",
  "Recognised text search stopped: {e}": "Die Suche im erkannten Text wurde abgebrochen: {e}",
  "the comparison stopped: {e}": "Der Vergleich wurde abgebrochen: {e}",
  "this link is no longer available": "Dieser Link ist nicht mehr verfügbar",
  "could not reach the system opener: {e}": "Das System konnte zum Öffnen nicht erreicht werden: {e}",
  "This field is no longer available": "Dieses Feld ist nicht mehr verfügbar",
  "Text no longer belongs to this page": "Der Text gehört nicht mehr zu dieser Seite",
  "This page is no longer in the document": "Diese Seite ist nicht mehr im Dokument",
  "The file this page came from is no longer open": "Die Datei, aus der diese Seite stammt, ist nicht mehr geöffnet",
  "This page has no editable original text": "Diese Seite hat keinen ursprünglichen Text, der sich bearbeiten ließe",

  // commands/session.rs
  "could not write the session file {path}: {e}": "Die Sitzungsdatei {path} konnte nicht geschrieben werden: {e}",
  "the session write did not run: {e}": "Das Schreiben der Sitzung ist nicht gelaufen: {e}",

  // commands/hidden.rs and hidden/survey.rs
  "The check was stopped before the last page, so there is no result.":
    "Die Prüfung wurde vor der letzten Seite angehalten, daher gibt es kein Ergebnis.",
  "Not looked at: comments, form values, attachments, metadata and earlier versions kept in the file.":
    "Nicht geprüft: Kommentare, Formularwerte, Anhänge, Metadaten und frühere Fassungen, die in der Datei aufbewahrt sind.",
  "comparing text with the pages": "Text mit den Seiten verglichen wurde",
  "The check did not run: {e}": "Die Prüfung ist nicht gelaufen: {e}",
  "{pages} too large to compare ({numbers}){rest}": (a) =>
    `${seiten(a.pages!)} zu groß zum Vergleichen (${a.numbers})${a.rest}`,
  "{pages} without text not checked ({numbers}){rest}": (a) =>
    `${seiten(a.pages!)} ohne Text nicht geprüft (${a.numbers})${a.rest}`,
  "{count} characters could not be judged{rest}": (a) =>
    `${a.count} Zeichen ${one(a.count!) ? "konnte" : "konnten"} nicht beurteilt werden${a.rest}`,
  "No hidden text found: {count} characters on {pages} compared with the rendered page{rest}": (a) =>
    `Kein verborgener Text gefunden: ${a.count} Zeichen auf ${seiten(a.pages!)} mit dem Seitenbild verglichen${a.rest}`,
  "1 passage is in the file and not visible on the page, on {pages}.": (a) =>
    `1 Textstelle steht in der Datei und ist auf der Seite nicht sichtbar, auf ${seiten(a.pages!)}.`,
  "1 passage is in the file and not visible on the page, on {pages}; {more}": (a) =>
    `1 Textstelle steht in der Datei und ist auf der Seite nicht sichtbar, auf ${seiten(a.pages!)}; ${a.more}`,
  "{count} passages are in the file and not visible on the page, on {pages}.": (a) =>
    `${a.count} Textstellen stehen in der Datei und sind auf der Seite nicht sichtbar, auf ${seiten(a.pages!)}.`,
  "{count} passages are in the file and not visible on the page, on {pages}; {more}": (a) =>
    `${a.count} Textstellen stehen in der Datei und sind auf der Seite nicht sichtbar, auf ${seiten(a.pages!)}; ${a.more}`,

  // commands/app.rs, commands/mod.rs, commands/menubar.rs (`the save did not run` is in save.ts;
  // the two page sentences of commands/hidden.rs are in workers.ts, with ocr.rs)
  "the forward search did not finish: {e}": "Die Vorwärtssuche wurde nicht beendet: {e}",
  "the command-line tool change did not run: {e}": "Die Änderung am Kommandozeilenwerkzeug ist nicht gelaufen: {e}",
  "the command-line tool could not be looked for: {e}":
    "Nach dem Kommandozeilenwerkzeug konnte nicht gesucht werden: {e}",
  "Explorer's handlers could not be read: {e}": "Die Erweiterungen des Explorers konnten nicht gelesen werden: {e}",
  "Explorer's handlers could not be changed: {e}": "Die Erweiterungen des Explorers konnten nicht geändert werden: {e}",
  "the default application change did not run: {e}": "Die Änderung der Standard-App ist nicht gelaufen: {e}",
  "the render service did not answer within {secs} s while {doing}":
    "Der Darstellungsdienst hat nicht innerhalb von {secs} s geantwortet, während {doing}",
  "the render service stopped": "Der Darstellungsdienst wurde beendet",
  "render thread stopped ({command})": "Der Darstellungs-Thread wurde beendet ({command})",
  "could not reach the main thread to read the keyboard: {e}":
    "Der Haupt-Thread konnte zum Lesen der Tastatur nicht erreicht werden: {e}",
  "the keyboard layout reader did not answer": "Das Lesen der Tastaturbelegung hat nicht geantwortet",
  "could not reach the web view to paste: {e}": "Die Webansicht konnte zum Einfügen nicht erreicht werden: {e}",

  // clitool.rs
  "{path} could not be read: {e}": "{path} konnte nicht gelesen werden: {e}",
  "{path} is a file that tpdf did not put there": "{path} ist eine Datei, die nicht von tpdf stammt",
  "{path} could not be read": "{path} konnte nicht gelesen werden",
  "{path} links to {target}, which is not tpdf's": "{path} verweist auf {target}, und das gehört nicht zu tpdf",
  "tpdf could not find where it is running from: {e}": "tpdf konnte nicht feststellen, von wo es läuft: {e}",
  "tpdf could not find the folder it is running from": "tpdf konnte den Ordner nicht finden, aus dem es läuft",
  "tpdf is running from a temporary location macOS chose, so a link to it would stop working. Move tpdf to the Applications folder, open it from there, and install the command-line tool again.":
    "tpdf läuft von einem vorübergehenden Ort, den macOS gewählt hat, daher würde ein Link darauf nicht mehr funktionieren. Verschiebe tpdf in den Ordner „Programme“, öffne es von dort und installiere das Kommandozeilenwerkzeug erneut.",
  "The command-line tool is not beside this copy of tpdf ({path}).":
    "Das Kommandozeilenwerkzeug liegt nicht neben diesem tpdf ({path}).",
  "Installed. `tpdf` in a terminal now runs the command-line tool ({LINK} links to {path}). Try `tpdf help`.":
    "Installiert. `tpdf` startet im Terminal jetzt das Kommandozeilenwerkzeug ({LINK} verweist auf {path}). Probier `tpdf help`.",
  "Removed {LINK} and {ALIAS}. The application is unchanged.":
    "{LINK} und {ALIAS} wurden entfernt. Die App selbst ist unverändert.",
  "{why}, so it was left alone.": "{why}, daher wurde es nicht angerührt.",
  "{path} was not installed.": "{path} wurde nicht installiert.",
  "{path} is still there.": "{path} ist noch vorhanden.",
  "Already installed: {LINK} links to {path}.": "Bereits installiert: {LINK} verweist auf {path}.",
  "The command-line tool is not installed ({LINK} does not exist).":
    "Das Kommandozeilenwerkzeug ist nicht installiert ({LINK} gibt es nicht).",
  "{why}, so it was left alone. Remove it yourself to install tpdf's.":
    "{why}, daher wurde es nicht angerührt. Entferne es selbst, um das von tpdf zu installieren.",
  "`tpdf-cli` runs it too, which is its name on Windows, so one script runs on both.":
    "`tpdf-cli` startet es ebenfalls; so heißt es unter Windows, damit ein Skript auf beiden läuft.",
  "the administrator prompt could not be shown: {e}": "Die Abfrage nach Administratorrechten konnte nicht angezeigt werden: {e}",
  "Nothing was changed: the administrator prompt was cancelled.":
    "Es wurde nichts geändert: Die Abfrage nach Administratorrechten wurde abgebrochen.",
  "Nothing was changed: {why}": "Es wurde nichts geändert: {why}",
  "the command-line tool is in no folder": "Das Kommandozeilenwerkzeug liegt in keinem Ordner",
  "Added {dir} to your PATH. Open a new terminal and run tpdf-cli.":
    "{dir} wurde zu deinem PATH hinzugefügt. Öffne ein neues Terminal und starte tpdf-cli.",
  "{dir} is already on your PATH: run tpdf-cli in a terminal.":
    "{dir} ist bereits in deinem PATH: Starte tpdf-cli in einem Terminal.",
  "Removed {dir} from your PATH. The tool itself is removed together with tpdf.":
    "{dir} wurde aus deinem PATH entfernt. Das Werkzeug selbst wird zusammen mit tpdf entfernt.",
  "{dir} is not on your PATH. The tool itself is removed together with tpdf.":
    "{dir} ist nicht in deinem PATH. Das Werkzeug selbst wird zusammen mit tpdf entfernt.",
  "The command-line tool is installed with tpdf, in {dir}.":
    "Das Kommandozeilenwerkzeug wird mit tpdf installiert, in {dir}.",
  "The command-line tool is removed together with tpdf; there is nothing to remove separately.":
    "Das Kommandozeilenwerkzeug wird zusammen mit tpdf entfernt; es gibt nichts gesondert zu entfernen.",

  // explorer.rs
  "PDF thumbnails are still {name}'s; turning it on again can replace it.":
    "PDF-Miniaturansichten kommen weiterhin von {name}; erneutes Einschalten kann das ersetzen.",
  "PDF thumbnails are still {name}'s; turning it on again can replace it. {rest}":
    "PDF-Miniaturansichten kommen weiterhin von {name}; erneutes Einschalten kann das ersetzen. {rest}",
  "The preview pane still is {name}'s; turning it on again can replace it.":
    "Das Vorschaufenster gehört weiterhin zu {name}; erneutes Einschalten kann das ersetzen.",
  "The preview pane still is {name}'s; turning it on again can replace it. {rest}":
    "Das Vorschaufenster gehört weiterhin zu {name}; erneutes Einschalten kann das ersetzen. {rest}",
  "Nothing changed.": "Es hat sich nichts geändert.",
  "Nothing changed. {rest}": "Es hat sich nichts geändert. {rest}",
  "Explorer now shows PDF thumbnails with tpdf.": "Der Explorer zeigt PDF-Miniaturansichten jetzt mit tpdf.",
  "Explorer now shows PDF thumbnails with tpdf. {rest}":
    "Der Explorer zeigt PDF-Miniaturansichten jetzt mit tpdf. {rest}",
  "Explorer now shows PDF the preview pane with tpdf.": "Der Explorer zeigt das PDF-Vorschaufenster jetzt mit tpdf.",
  "Explorer now shows PDF the preview pane with tpdf. {rest}":
    "Der Explorer zeigt das PDF-Vorschaufenster jetzt mit tpdf. {rest}",
  "Explorer now shows PDF thumbnails and the preview pane with tpdf.":
    "Der Explorer zeigt PDF-Miniaturansichten und das Vorschaufenster jetzt mit tpdf.",
  "Explorer now shows PDF thumbnails and the preview pane with tpdf. {rest}":
    "Der Explorer zeigt PDF-Miniaturansichten und das Vorschaufenster jetzt mit tpdf. {rest}",
  "PDF thumbnails are {name}'s again.": "PDF-Miniaturansichten kommen wieder von {name}.",
  "PDF thumbnails are {name}'s again. {rest}": "PDF-Miniaturansichten kommen wieder von {name}. {rest}",
  "The preview pane is {name}'s again.": "Das Vorschaufenster gehört wieder zu {name}.",
  "The preview pane is {name}'s again. {rest}": "Das Vorschaufenster gehört wieder zu {name}. {rest}",
  "Explorer no longer uses tpdf for PDFs.": "Der Explorer verwendet tpdf nicht mehr für PDFs.",
  "Explorer no longer uses tpdf for PDFs. {rest}": "Der Explorer verwendet tpdf nicht mehr für PDFs. {rest}",
  "where tpdf runs from has no folder": "Der Ort, von dem tpdf läuft, hat keinen Ordner",
  "{path} is not there, so Explorer has nothing of tpdf's to load; reinstall tpdf":
    "{path} fehlt, daher kann der Explorer nichts von tpdf laden; installiere tpdf neu",
  "the registry could not read {key} (error {status})": "Die Registrierung konnte {key} nicht lesen (Fehler {status})",
  "the registry could not write {key} (error {status})": "Die Registrierung konnte {key} nicht schreiben (Fehler {status})",
  "the registry could not remove from {key} (error {status})":
    "Die Registrierung konnte nichts aus {key} entfernen (Fehler {status})",
  "the registry could not remove {key} (error {status})": "Die Registrierung konnte {key} nicht entfernen (Fehler {status})",
  "the registry could not open {key} (error {status})": "Die Registrierung konnte {key} nicht öffnen (Fehler {status})",
  "where tpdf runs from: {e}": "Der Ort, von dem tpdf läuft: {e}",
  "Explorer is the Windows file manager; on macOS, Finder and Quick Look show PDFs themselves":
    "Der Explorer ist der Dateimanager von Windows; unter macOS zeigen Finder und Übersicht PDFs selbst an",
  "Explorer is the Windows file manager; on Linux, the file manager's own thumbnailer shows PDFs":
    "Der Explorer ist der Dateimanager von Windows; unter Linux zeigt der Dateimanager PDFs mit seinen eigenen Miniaturansichten",

  // lib.rs
  "{message} --- and the document did not close cleanly: {also}":
    "{message} – und das Dokument wurde nicht sauber geschlossen: {also}",

  // userpath.rs
  "your PATH could not be opened (error {status})": "Dein PATH konnte nicht geöffnet werden (Fehler {status})",
  "your PATH could not be read (error {status})": "Dein PATH konnte nicht gelesen werden (Fehler {status})",
  "your PATH is not stored as text, so tpdf leaves it alone":
    "Dein PATH ist nicht als Text gespeichert, daher lässt tpdf ihn unverändert",
  "your PATH is too long": "Dein PATH ist zu lang",
  "your PATH holds text tpdf cannot read, so tpdf leaves it alone":
    "Dein PATH enthält Text, den tpdf nicht lesen kann, daher lässt tpdf ihn unverändert",
  "your PATH could not be written (error {status})": "Dein PATH konnte nicht geschrieben werden (Fehler {status})",

  // defaultapp.rs
  'To change that, select a PDF in Finder, choose Get Info, and pick another application under "Open with".':
    "Um das zu ändern, wähle im Finder ein PDF aus, dann „Informationen“, und such dir unter „Öffnen mit“ eine andere App aus.",
  'To change that, open a PDF\'s properties in your file manager and choose another application under "Open With".':
    "Um das zu ändern, öffne im Dateimanager die Eigenschaften eines PDFs und wähle unter „Öffnen mit“ eine andere App.",
  "tpdf already opens PDF documents.": "tpdf öffnet PDF-Dokumente bereits.",
  "tpdf now opens PDF documents. {change}": "tpdf öffnet jetzt PDF-Dokumente. {change}",
  "tpdf is not the default: PDF documents still open with {handler}":
    "tpdf ist nicht die Standard-App: PDF-Dokumente öffnen sich weiterhin mit {handler}",
  "tpdf is not the default: the system names no application for PDF documents":
    "tpdf ist nicht die Standard-App: Das System nennt keine App für PDF-Dokumente",
  "this copy of tpdf is not an application: open tpdf from the Applications folder":
    "Dieses tpdf ist keine App: Öffne tpdf aus dem Ordner „Programme“",
  "a sample document: {e}": "Ein Beispieldokument: {e}",
  "{still}. The system's question was not confirmed.": "{still}. Die Frage des Systems wurde nicht bestätigt.",
  "Settings did not open. Choose tpdf for .pdf under Apps, Default apps.":
    "Die Einstellungen haben sich nicht geöffnet. Wähle tpdf für .pdf unter Apps, Standard-Apps.",
  "Windows lets only you choose the default. Settings is open at Default apps: choose tpdf, then set it for .pdf.":
    "Unter Windows kannst nur du die Standard-App wählen. Die Einstellungen sind bei Standard-Apps geöffnet: Wähle tpdf und lege es für .pdf fest.",
  "this copy of tpdf is not installed, so there is no tpdf.desktop to name: install the tpdf package and run it from the applications menu":
    "Dieses tpdf ist nicht installiert, daher gibt es kein tpdf.desktop, das sich nennen ließe: Installiere das tpdf-Paket und starte es aus dem Anwendungsmenü",
  "tpdf is not the default: {e}": "tpdf ist nicht die Standard-App: {e}",

  // protect.rs
  "The password is empty; a document protected with it would open for anybody.":
    "Das Passwort ist leer; ein damit geschütztes Dokument würde sich für jeden öffnen.",
  "The password is too long: a PDF password is at most {MAX_BYTES} bytes, and this one is {length}.":
    "Das Passwort ist zu lang: Ein PDF-Passwort hat höchstens {MAX_BYTES} Bytes, und dieses hat {length}.",
  "The password holds a control character, such as a tab or a line break, which most readers cannot type back.":
    "Das Passwort enthält ein Steuerzeichen, etwa einen Tabulator oder einen Zeilenumbruch, das die meisten Leser nicht wieder eintippen können.",
  "This document has no password, so there is none to remove.":
    "Dieses Dokument hat kein Passwort, daher gibt es keins zu entfernen.",
  "This document opens without a password. What it restricts was set by whoever made it, and tpdf removes a password only from a document that needs one to open.":
    "Dieses Dokument öffnet sich ohne Passwort. Was es einschränkt, hat festgelegt, wer es erstellt hat, und tpdf entfernt ein Passwort nur von einem Dokument, das zum Öffnen eines braucht.",
  "tpdf could not get random bytes for the encryption key: {e}":
    "tpdf konnte keine Zufallsbytes für den Schlüssel der Verschlüsselung bekommen: {e}",
  "tpdf could not use this password: {e}": "tpdf konnte dieses Passwort nicht verwenden: {e}",
  "tpdf could not complete the encryption it wrote: {e}":
    "tpdf konnte die geschriebene Verschlüsselung nicht vervollständigen: {e}",
  "tpdf could not get random bytes for the file identifier: {e}":
    "tpdf konnte keine Zufallsbytes für die Dateikennung bekommen: {e}",
  "tpdf could not read back the copy it built: {e}": "tpdf konnte die erstellte Kopie nicht zurücklesen: {e}",
  "The copy tpdf built is still encrypted, so it was not written.":
    "Die von tpdf erstellte Kopie ist noch verschlüsselt, daher wurde sie nicht geschrieben.",
  "The copy tpdf built opens without the password, so it was not written.":
    "Die von tpdf erstellte Kopie öffnet sich ohne das Passwort, daher wurde sie nicht geschrieben.",
  "The copy tpdf built does not open with the password, so it was not written.":
    "Die von tpdf erstellte Kopie öffnet sich nicht mit dem Passwort, daher wurde sie nicht geschrieben.",

  // compress.rs
  "the resolution is {dpi} pixels an inch; it may be {low} to {high}":
    "Die Auflösung beträgt {dpi} Pixel pro Zoll; erlaubt sind {low} bis {high}",
  "the JPEG quality is {quality}; it may be 1 to 100": "Die JPEG-Qualität beträgt {quality}; erlaubt sind 1 bis 100",
  "the smaller copy": "die kleinere Kopie",

  // synctex.rs
  "there is no SyncTeX file beside {path}; compile it with -synctex=1":
    "Neben {path} liegt keine SyncTeX-Datei; übersetze es mit -synctex=1",
  "nothing on any page came from {name}": "Nichts auf irgendeiner Seite stammt aus {name}",
  "could not read {path}: {e}": "{path} konnte nicht gelesen werden: {e}",
  "{path} is {size} MB, more than the {most} MB tpdf reads":
    "{path} hat {size} MB, mehr als die {most} MB, die tpdf liest",
  "the SyncTeX file could not be read: {e}": "Die SyncTeX-Datei konnte nicht gelesen werden: {e}",
  "the SyncTeX file is more than {most} MB decompressed": "Die SyncTeX-Datei hat entpackt mehr als {most} MB",
  "the SyncTeX file has a line longer than 64 KB": "Die SyncTeX-Datei hat eine Zeile, die länger als 64 KB ist",
  "{path} names no file": "{path} nennt keine Datei",
  "the SyncTeX file names no input called {name}": "Die SyncTeX-Datei nennt keine Eingabe namens {name}",
  "the SyncTeX file names {count} different inputs called {name}; give the source's full path":
    "Die SyncTeX-Datei nennt {count} verschiedene Eingaben namens {name}; gib den vollständigen Pfad der Quelle an",
  "the SyncTeX file nests boxes more than {MAX_DEPTH} deep":
    "Die SyncTeX-Datei verschachtelt Boxen tiefer als {MAX_DEPTH} Ebenen",

  // fingerprint.rs
  "could not open {path} to fingerprint it: {e}":
    "{path} konnte nicht geöffnet werden, um den Fingerabdruck zu bilden: {e}",
  "could not read {path}'s metadata: {e}": "Die Metadaten von {path} konnten nicht gelesen werden: {e}",
  "could not read {path} to fingerprint it: {e}":
    "{path} konnte nicht gelesen werden, um den Fingerabdruck zu bilden: {e}",
  "{path} could not be checked before saving: {e}": "{path} konnte vor dem Speichern nicht geprüft werden: {e}",
  "it was modified": "sie wurde geändert",
  "its contents changed while keeping the same length": "ihr Inhalt hat sich bei gleicher Länge geändert",
  "its length went from {was} to {now}": "ihre Länge hat sich von {was} auf {now} geändert",
  "{path} changed on disk since you opened it --- {how}":
    "{path} wurde auf dem Datenträger geändert, seit du es geöffnet hast – {how}",
  "{fact}. Your edits are still here: save them under another name, or open the file again to start from what is on disk now.":
    "{fact}. Deine Änderungen sind noch da: Speichere sie unter einem anderen Namen, oder öffne die Datei erneut, um mit dem zu beginnen, was jetzt auf dem Datenträger liegt.",

  // menu.rs
  "could not reach the main thread to build the menu: {e}":
    "Der Haupt-Thread konnte zum Aufbau des Menüs nicht erreicht werden: {e}",
  "the menu builder did not answer": "Der Aufbau des Menüs hat nicht geantwortet",
  "could not update {count} menu item(s): {items}": (a) =>
    `${a.count} ${a.count === "1" ? "Menüeintrag konnte" : "Menüeinträge konnten"} nicht aktualisiert werden: ${a.items}`,
  "could not reach the main thread to update the menu: {e}":
    "Der Haupt-Thread konnte zum Aktualisieren des Menüs nicht erreicht werden: {e}",
  "the menu updater did not answer": "Das Aktualisieren des Menüs hat nicht geantwortet",
  "could not build the menu item {title:?} (accelerator {accelerator:?}): {e}":
    "Der Menüeintrag {title} konnte nicht angelegt werden (Tastenkürzel {accelerator}): {e}",
  "could not build the menu: {e}": "Das Menü konnte nicht aufgebaut werden: {e}",

  // search.rs, opener.rs, compare.rs, imports.rs, encoding.rs, document.rs
  "not a valid pattern": "kein gültiges Muster",
  "this link is not an address the system could read": "Dieser Link ist keine Adresse, die das System lesen kann",
  "the system could not open this link": "Das System konnte diesen Link nicht öffnen",
  "the system could not open this link: {e}": "Das System konnte diesen Link nicht öffnen: {e}",
  "the {which} document has more than {MAX_WORDS} words, which is more than tpdf compares": (a) =>
    `Das ${SIDE[a.which!] ?? a.which} Dokument hat mehr als ${a.MAX_WORDS} Wörter, mehr als tpdf vergleicht`,
  "{name} is encrypted, and tpdf cannot insert pages from an encrypted file yet. Save an unencrypted copy of it first.":
    "{name} ist verschlüsselt, und tpdf kann noch keine Seiten aus einer verschlüsselten Datei einfügen. Speichere zuerst eine unverschlüsselte Kopie davon.",
  "a file of {count} pages is past what tpdf can insert":
    "Eine Datei mit {count} Seiten übersteigt, was tpdf einfügen kann",
  "could not parse the document: {e}": "Das Dokument konnte nicht gelesen werden: {e}",
  "text preview is unavailable": "Die Textvorschau ist nicht verfügbar",
} satisfies Record<string, Wording>;

/** The entries above that say "du", addressing the reader with "Sie" instead. */
export const sie = {
  "tpdf is running from a temporary location macOS chose, so a link to it would stop working. Move tpdf to the Applications folder, open it from there, and install the command-line tool again.":
    "tpdf läuft von einem vorübergehenden Ort, den macOS gewählt hat, daher würde ein Link darauf nicht mehr funktionieren. Verschieben Sie tpdf in den Ordner „Programme“, öffnen Sie es von dort und installieren Sie das Kommandozeilenwerkzeug erneut.",
  "{why}, so it was left alone. Remove it yourself to install tpdf's.":
    "{why}, daher wurde es nicht angerührt. Entfernen Sie es selbst, um das von tpdf zu installieren.",
  "Added {dir} to your PATH. Open a new terminal and run tpdf-cli.":
    "{dir} wurde zu Ihrem PATH hinzugefügt. Öffnen Sie ein neues Terminal und starten Sie tpdf-cli.",
  "{dir} is already on your PATH: run tpdf-cli in a terminal.":
    "{dir} ist bereits in Ihrem PATH: Starten Sie tpdf-cli in einem Terminal.",
  "Removed {dir} from your PATH. The tool itself is removed together with tpdf.":
    "{dir} wurde aus Ihrem PATH entfernt. Das Werkzeug selbst wird zusammen mit tpdf entfernt.",
  "{dir} is not on your PATH. The tool itself is removed together with tpdf.":
    "{dir} ist nicht in Ihrem PATH. Das Werkzeug selbst wird zusammen mit tpdf entfernt.",
  "{path} is not there, so Explorer has nothing of tpdf's to load; reinstall tpdf":
    "{path} fehlt, daher kann der Explorer nichts von tpdf laden; installieren Sie tpdf neu",
  "your PATH could not be opened (error {status})":
    "Ihr PATH konnte nicht geöffnet werden (Fehler {status})",
  "your PATH could not be read (error {status})":
    "Ihr PATH konnte nicht gelesen werden (Fehler {status})",
  "your PATH is not stored as text, so tpdf leaves it alone":
    "Ihr PATH ist nicht als Text gespeichert, daher lässt tpdf ihn unverändert",
  "your PATH is too long":
    "Ihr PATH ist zu lang",
  "your PATH holds text tpdf cannot read, so tpdf leaves it alone":
    "Ihr PATH enthält Text, den tpdf nicht lesen kann, daher lässt tpdf ihn unverändert",
  "your PATH could not be written (error {status})":
    "Ihr PATH konnte nicht geschrieben werden (Fehler {status})",
  'To change that, select a PDF in Finder, choose Get Info, and pick another application under "Open with".':
    "Um das zu ändern, wählen Sie im Finder ein PDF aus, dann „Informationen“, und suchen Sie sich unter „Öffnen mit“ eine andere App aus.",
  'To change that, open a PDF\'s properties in your file manager and choose another application under "Open With".':
    "Um das zu ändern, öffnen Sie im Dateimanager die Eigenschaften eines PDFs und wählen Sie unter „Öffnen mit“ eine andere App.",
  "this copy of tpdf is not an application: open tpdf from the Applications folder":
    "Dieses tpdf ist keine App: Öffnen Sie tpdf aus dem Ordner „Programme“",
  "Settings did not open. Choose tpdf for .pdf under Apps, Default apps.":
    "Die Einstellungen haben sich nicht geöffnet. Wählen Sie tpdf für .pdf unter Apps, Standard-Apps.",
  "Windows lets only you choose the default. Settings is open at Default apps: choose tpdf, then set it for .pdf.":
    "Unter Windows können nur Sie die Standard-App wählen. Die Einstellungen sind bei Standard-Apps geöffnet: Wählen Sie tpdf und legen Sie es für .pdf fest.",
  "this copy of tpdf is not installed, so there is no tpdf.desktop to name: install the tpdf package and run it from the applications menu":
    "Dieses tpdf ist nicht installiert, daher gibt es kein tpdf.desktop, das sich nennen ließe: Installieren Sie das tpdf-Paket und starten Sie es aus dem Anwendungsmenü",
  "the SyncTeX file names {count} different inputs called {name}; give the source's full path":
    "Die SyncTeX-Datei nennt {count} verschiedene Eingaben namens {name}; geben Sie den vollständigen Pfad der Quelle an",
  "{path} changed on disk since you opened it --- {how}":
    "{path} wurde auf dem Datenträger geändert, seit Sie es geöffnet haben – {how}",
  "{fact}. Your edits are still here: save them under another name, or open the file again to start from what is on disk now.":
    "{fact}. Ihre Änderungen sind noch da: Speichern Sie sie unter einem anderen Namen, oder öffnen Sie die Datei erneut, um mit dem zu beginnen, was jetzt auf dem Datenträger liegt.",
  "{name} is encrypted, and tpdf cannot insert pages from an encrypted file yet. Save an unencrypted copy of it first.":
    "{name} ist verschlüsselt, und tpdf kann noch keine Seiten aus einer verschlüsselten Datei einfügen. Speichern Sie zuerst eine unverschlüsselte Kopie davon.",
} satisfies Record<string, Wording>;
