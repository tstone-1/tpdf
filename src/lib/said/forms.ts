/** Forms, fields, marks and the edit model: `forms.rs`, `formfields.rs`,
 * `formedit.rs`, `edits.rs`, `docmodel.rs`, and the picture, region,
 * attachment and image-page commands beside them. */

import type { Wording } from "./de";

/** The kinds `formfields::placed` names, in German. */
const KIND: Record<string, string> = {
  checkbox: "Kontrollkästchen",
  "radio button": "Optionsfeld",
  "signature field": "Signaturfeld",
  "text field": "Textfeld",
};

/** `n` pages, with the noun bent to the number. */
const pages = (n: string): string => (n === "1" ? "1 Seite" : `${n} Seiten`);

export default {
  // formfields.rs
  "`{name}`: a text size is {least} to {most} points, and is left out for one that follows the field":
    "`{name}`: Eine Schriftgröße liegt zwischen {least} und {most} Punkt und entfällt bei einer, die sich dem Feld anpasst",
  "`{name}`: a signature field has no text to size": "`{name}`: Ein Signaturfeld hat keinen Text, dessen Größe sich einstellen ließe",
  "`{name}`: a box or a button has no text to size":
    "`{name}`: Ein Kästchen oder eine Schaltfläche hat keinen Text, dessen Größe sich einstellen ließe",
  "`{name}`: only a text field has a default value": "`{name}`: Nur ein Textfeld hat einen Standardwert",
  "`{name}` takes one line, and its default value has several":
    "`{name}` nimmt eine Zeile, und sein Standardwert hat mehrere",
  "`{name}`: the default value has {holds} characters, which is more than the field takes":
    "`{name}`: Der Standardwert hat {holds} Zeichen, mehr als das Feld aufnimmt",
  "`{name}`: its default value: {why}": "`{name}`: sein Standardwert: {why}",
  "`{name}`: only a text field has a most characters": "`{name}`: Nur ein Textfeld hat eine Höchstzahl an Zeichen",
  "`{name}`: the most characters is a number from 1 to 16384":
    "`{name}`: Die Höchstzahl an Zeichen ist eine Zahl von 1 bis 16384",
  "`{name}`: a box or a button has no text to align":
    "`{name}`: Ein Kästchen oder eine Schaltfläche hat keinen Text, der sich ausrichten ließe",
  "`{name}`: a signature field has no text to align": "`{name}`: Ein Signaturfeld hat keinen Text, der sich ausrichten ließe",
  "`{name}`: a signature field that is read-only could not be signed":
    "`{name}`: Ein schreibgeschütztes Signaturfeld könnte nicht signiert werden",
  "`{name}`: a radio button has one value": "`{name}`: Ein Optionsfeld hat einen Wert",
  "`{name}`: `Off` is what a group holds when nothing is chosen, so no button can have it as its value":
    "`{name}`: `Off` hält eine Gruppe, wenn nichts gewählt ist, deshalb kann kein Optionsfeld diesen Wert haben",
  "`{name}`: only a dropdown has choices": "`{name}`: Nur eine Auswahlliste hat Einträge",
  "`{name}`: a dropdown needs at least one choice": "`{name}`: Eine Auswahlliste braucht mindestens einen Eintrag",
  "`{name}`: {count} choices is more than the {MAX_OPTIONS} a dropdown offers":
    "`{name}`: {count} Einträge sind mehr als die {MAX_OPTIONS}, die eine Auswahlliste anbietet",
  "`{name}`: a choice cannot be empty": "`{name}`: Ein Eintrag darf nicht leer sein",
  "`{name}`: the choice `{option}` begins or ends with a space":
    "`{name}`: Der Eintrag `{option}` beginnt oder endet mit einem Leerzeichen",
  "`{name}`: a choice cannot contain a control character": "`{name}`: Ein Eintrag darf kein Steuerzeichen enthalten",
  "`{name}`: a choice is at most {MAX_OPTION} characters": "`{name}`: Ein Eintrag hat höchstens {MAX_OPTION} Zeichen",
  "`{name}`: the choice `{option}` is there twice": "`{name}`: Den Eintrag `{option}` gibt es zweimal",
  "a field needs a name": "Ein Feld braucht einen Namen",
  "the name `{name}` begins or ends with a space": "Der Name `{name}` beginnt oder endet mit einem Leerzeichen",
  "the name `{name}` contains a period, which a form uses to separate a field from the group it is in":
    "Der Name `{name}` enthält einen Punkt, mit dem ein Formular ein Feld von seiner Gruppe trennt",
  "a field's name cannot contain a control character": "Der Name eines Felds darf kein Steuerzeichen enthalten",
  "a field's name is at most {MAX_NAME} characters": "Der Name eines Felds hat höchstens {MAX_NAME} Zeichen",
  "`{name}`: its rectangle is not four numbers": "`{name}`: Sein Rechteck besteht nicht aus vier Zahlen",
  "`{name}`: {width} by {height} points is too small --- a {kind} needs at least {least} by {least_high}": (a) =>
    `\`${a.name}\`: ${a.width} mal ${a.height} Punkt ist zu klein – ein ${KIND[a.kind!] ?? a.kind} braucht mindestens ${a.least} mal ${a.least_high}`,
  "`{name}`: its rectangle is not inside page {page}, which is {width} by {height} points":
    "`{name}`: Sein Rechteck liegt nicht auf Seite {page}, die {width} mal {height} Punkt groß ist",
  "{count} fields is more than the {MAX_NEW} one call adds":
    "{count} Felder sind mehr als die {MAX_NEW}, die ein Aufruf hinzufügt",
  "`{name}`: the form already has a field of this name": "`{name}`: Das Formular hat schon ein Feld mit diesem Namen",
  "`{name}` is named more than once": "`{name}` wird mehr als einmal genannt",
  "`{name}`: the group already has a button with the value `{value}`":
    "`{name}`: Die Gruppe hat schon ein Optionsfeld mit dem Wert `{value}`",
  "`{name}`: a radio button takes no characters, so it has no most":
    "`{name}`: Ein Optionsfeld nimmt keine Zeichen auf und hat deshalb keine Höchstzahl",
  "`{name}`: a checkbox takes no characters, so it has no most":
    "`{name}`: Ein Kontrollkästchen nimmt keine Zeichen auf und hat deshalb keine Höchstzahl",
  "`{name}`: a signature field takes no characters, so it has no most":
    "`{name}`: Ein Signaturfeld nimmt keine Zeichen auf und hat deshalb keine Höchstzahl",
  "`{name}`: a dropdown takes one of its choices, so it has no most characters":
    "`{name}`: Eine Auswahlliste nimmt einen ihrer Einträge auf und hat deshalb keine Höchstzahl an Zeichen",
  "`{name}`: a tooltip is at most 1024 characters": "`{name}`: Ein Tooltip hat höchstens 1024 Zeichen",
  "`{name}`: there is no page {page}; the document has {count}": (a) =>
    `\`${a.name}\`: Es gibt keine Seite ${a.page}; das Dokument hat ${pages(a.count!)}`,
  "the document's form is not a dictionary": "Das Formular des Dokuments ist kein Dictionary",
  "/{key} is not a list": "/{key} ist keine Liste",
  "`{name}`: a field of this kind needs at least {least} by {least_high} points":
    "`{name}`: Ein Feld dieser Art braucht mindestens {least} mal {least_high} Punkt",
  "a signature field's rectangle is not four numbers": "Das Rechteck eines Signaturfelds besteht nicht aus vier Zahlen",

  // forms.rs
  "A choice option is not a text string": "Ein Eintrag einer Auswahl ist keine Zeichenkette",
  "This form contains an oversized option": "Dieses Formular enthält einen zu großen Eintrag",
  "Invalid choice options": "Ungültige Einträge einer Auswahl",
  "This form exceeds the option limit": "Dieses Formular hat mehr Einträge, als tpdf liest",
  "Invalid choice export/label pair": "Ungültiges Paar aus Exportwert und Beschriftung in einer Auswahl",
  "This form exceeds the option text limit": "Dieses Formular hat mehr Eintragstext, als tpdf liest",
  "This form exceeds the selection limit": "Dieses Formular hat mehr gewählte Einträge, als tpdf liest",
  "A choice with duplicate export values has no valid selection indices":
    "Eine Auswahl mit doppelten Exportwerten hat keine gültige Auswahl",
  "The saved choice is absent from its options": "Die gespeicherte Auswahl fehlt unter ihren Einträgen",
  "A radio button has no appearance states": "Ein Optionsfeld hat keine Zustände für sein Erscheinungsbild",
  "A radio button has ambiguous appearance states": "Ein Optionsfeld hat mehrdeutige Zustände für sein Erscheinungsbild",
  "A radio button is missing an appearance": "Einem Optionsfeld fehlt ein Erscheinungsbild",
  "A radio button appearance is not a stream": "Das Erscheinungsbild eines Optionsfelds ist kein Stream",
  "XFA forms are not supported": "XFA-Formulare werden nicht unterstützt",
  "This form exceeds the field-tree limit": "Der Feldbaum dieses Formulars ist größer, als tpdf liest",
  "This form exceeds the annotation limit": "Dieses Formular hat mehr Anmerkungen, als tpdf liest",
  "A widget has no field": "Ein Widget gehört zu keinem Feld",
  "A widget is outside the form field tree": "Ein Widget liegt außerhalb des Feldbaums des Formulars",
  "This form contains an oversized answer": "Dieses Formular enthält eine zu große Antwort",
  "A form widget has no rectangle": "Ein Formular-Widget hat kein Rechteck",
  "Invalid form widget rectangle": "Ungültiges Rechteck eines Formular-Widgets",
  "Invalid form widget coordinate": "Ungültige Koordinate eines Formular-Widgets",
  "Invalid form widget bounds": "Ungültige Grenzen eines Formular-Widgets",
  "Cyclic form field ancestry": "Die Formularfelder verweisen im Kreis aufeinander",
  "Unreadable form field": "Unlesbares Formularfeld",
  "This form contains an oversized field name": "Dieses Formular enthält einen zu langen Feldnamen",
  "This form exceeds the text limit": "Dieses Formular hat mehr Text, als tpdf liest",
  "This field type is not supported yet": "Diese Feldart wird noch nicht unterstützt",
  "This field is read-only": "Dieses Feld ist schreibgeschützt",
  "Password fields are not supported yet": "Passwortfelder werden noch nicht unterstützt",
  "File-select fields are not supported yet": "Dateiauswahlfelder werden noch nicht unterstützt",
  "Comb fields are not supported yet": "Kammfelder werden noch nicht unterstützt",
  "Rich-text fields are not supported yet": "Rich-Text-Felder werden noch nicht unterstützt",
  "This field is hidden": "Dieses Feld ist ausgeblendet",
  "This form exceeds the widget limit": "Dieses Formular hat mehr Widgets, als tpdf liest",
  "Another program saved this group with its answer drawn in, so it can be read and not changed":
    "Ein anderes Programm hat diese Gruppe mit eingezeichneter Antwort gespeichert, deshalb lässt sie sich lesen, aber nicht ändern",
  "Choose one available radio button": "Wähle ein vorhandenes Optionsfeld",
  "Choose available options without duplicates": "Wähle vorhandene Einträge, jeden nur einmal",
  "This option uses characters that cannot be saved visibly":
    "Dieser Eintrag enthält Zeichen, die sich nicht sichtbar speichern lassen",
  "The answer does not match the field type": "Die Antwort passt nicht zur Feldart",
  "A form answer is limited to 16 KB": "Eine Formularantwort hat höchstens 16 KB",
  "This answer exceeds the field's maximum length": "Diese Antwort ist länger, als das Feld zulässt",
  "This field supports Western European characters only": "Dieses Feld nimmt nur westeuropäische Zeichen auf",
  "This field accepts one line only": "Dieses Feld nimmt nur eine Zeile auf",
  "The options do not fit visibly in this field": "Die Einträge passen nicht sichtbar in dieses Feld",
  "This list uses characters that cannot be saved visibly":
    "Diese Liste enthält Zeichen, die sich nicht sichtbar speichern lassen",
  "The answer does not fit visibly in this field": "Die Antwort passt nicht sichtbar in dieses Feld",
  "Duplicate form answer": "Doppelte Formularantwort",
  "The form field is no longer in this document": "Das Formularfeld ist nicht mehr in diesem Dokument",
  "Shared checkboxes have incompatible on states": "Verbundene Kontrollkästchen haben unvereinbare Ein-Zustände",

  // formedit.rs
  "`{name}`: a tooltip is at most {MAX_TOOLTIP} characters": "`{name}`: Ein Tooltip hat höchstens {MAX_TOOLTIP} Zeichen",
  "`{name}`: a tooltip cannot contain a control character": "`{name}`: Ein Tooltip darf kein Steuerzeichen enthalten",
  "`{name}`: the most characters is a number up to {MAX_LENGTH}":
    "`{name}`: Die Höchstzahl an Zeichen ist eine Zahl bis {MAX_LENGTH}",
  "`{name}`: a text size is {least} to {most} points, or nought for one that follows the field":
    "`{name}`: Eine Schriftgröße liegt zwischen {least} und {most} Punkt, oder null für eine, die sich dem Feld anpasst",
  "`{name}`: a default value is at most {MAX_LENGTH} characters": "`{name}`: Ein Standardwert hat höchstens {MAX_LENGTH} Zeichen",
  "`{name}`: a default value cannot contain a control character":
    "`{name}`: Ein Standardwert darf kein Steuerzeichen enthalten",
  "`{name}`: only a radio button has a value it gives its group":
    "`{name}`: Nur ein Optionsfeld hat einen Wert, den es seiner Gruppe gibt",
  "`{name}` keeps its value: {reason}": "`{name}` behält seinen Wert: {reason}",
  "`{name}` keeps its value: the group lists what its buttons export, which tpdf does not rewrite":
    "`{name}` behält seinen Wert: Die Gruppe führt auf, was ihre Optionsfelder exportieren, und das schreibt tpdf nicht um",
  "a radio button is not among its group's states": "Ein Optionsfeld ist nicht unter den Zuständen seiner Gruppe",
  "`{name}` keeps its value: another button of the group has the same one, and they are chosen together":
    "`{name}` behält seinen Wert: Ein anderes Optionsfeld der Gruppe hat denselben, und beide werden zusammen gewählt",
  "`{name}`: a field of this kind is at least {least} by {least_high} points":
    "`{name}`: Ein Feld dieser Art ist mindestens {least} mal {least_high} Punkt groß",
  "`{name}`: its page is no longer in this document": "`{name}`: Seine Seite ist nicht mehr in diesem Dokument",
  "the field's page is no longer in this document": "Die Seite des Felds ist nicht mehr in diesem Dokument",
  "the field's page lists no annotations": "Die Seite des Felds führt keine Anmerkungen",
  "the page's annotations could not be changed": "Die Anmerkungen der Seite konnten nicht geändert werden",
  "the field lists no widgets": "Das Feld führt keine Widgets",
  "the field's widgets could not be changed": "Die Widgets des Felds konnten nicht geändert werden",
  "the form's field list could not be found": "Die Feldliste des Formulars wurde nicht gefunden",
  "the form's field list could not be changed": "Die Feldliste des Formulars konnte nicht geändert werden",
  "`{name}` holds {holds} characters, which is more than the {most} it would take":
    "`{name}` enthält {holds} Zeichen, mehr als die {most}, die es aufnehmen würde",
  "`{name}`: only a text field or a field of choices has text to align":
    "`{name}`: Nur ein Textfeld oder ein Auswahlfeld hat Text, der sich ausrichten lässt",
  "`{name}`: only a field of choices has choices": "`{name}`: Nur ein Auswahlfeld hat Einträge",
  "`{name}`: only a text field or a field of choices has a text size":
    "`{name}`: Nur ein Textfeld oder ein Auswahlfeld hat eine Schriftgröße",
  "`{name}` keeps its alignment, text size, default value and choices: {reason}, so tpdf cannot redraw it":
    "`{name}` behält Ausrichtung, Schriftgröße, Standardwert und Einträge: {reason}, deshalb kann tpdf es nicht neu zeichnen",
  "{count} field changes is more than the {MAX_EDITS} one save makes":
    "{count} Feldänderungen sind mehr als die {MAX_EDITS}, die ein Speichern vornimmt",
  "a form field is changed twice": "Ein Formularfeld wird zweimal geändert",
  "`{name}` is a signed signature field, which tpdf does not move, rename or remove":
    "`{name}` ist ein signiertes Signaturfeld, das tpdf nicht verschiebt, umbenennt oder entfernt",
  "`{name}` is given two sets of properties": "`{name}` bekommt zwei verschiedene Eigenschaften",
  "`{name}` is given two names": "`{name}` bekommt zwei Namen",
  "`{name}` can be moved and not resized: {reason}, so tpdf cannot redraw it":
    "`{name}` lässt sich verschieben, aber nicht in der Größe ändern: {reason}, deshalb kann tpdf es nicht neu zeichnen",
  "`{name}`: another field has this name": "`{name}`: Ein anderes Feld hat diesen Namen",

  // edits.rs
  "a field of this kind is at least {least} by {least_high} points":
    "Ein Feld dieser Art ist mindestens {least} mal {least_high} Punkt groß",
  "a page cannot be {bad} points": "Eine Seite kann nicht {bad} Punkt groß sein",
  "the file chosen for that insert is no longer open ({pending})":
    "Die für dieses Einfügen gewählte Datei ist nicht mehr geöffnet ({pending})",
  "a mark is four numbers per rectangle, and this has {count}":
    "Eine Markierung hat vier Zahlen je Rechteck, und diese hat {count}",
  "a stroke is two numbers per point, and one of these has {count}":
    "Ein Strich hat zwei Zahlen je Punkt, und einer dieser hat {count}",
  "a mark cannot have a corner at {bad}": "Eine Markierung kann keine Ecke bei {bad} haben",
  "The picture is empty, invalid or too large": "Das Bild ist leer, ungültig oder zu groß",
  "The signature image is empty, invalid or too large": "Das Unterschriftsbild ist leer, ungültig oder zu groß",
  "a signature field is at least {least} by {least_high} points; drag a larger rectangle":
    "Ein Signaturfeld ist mindestens {least} mal {least_high} Punkt groß; zieh ein größeres Rechteck",
  "a redaction cannot have a corner at {bad}": "Eine Schwärzung kann keine Ecke bei {bad} haben",
  "stroke {past} of a drawing that has {held}, so this gesture was aimed at something else":
    "Strich {past} einer Zeichnung mit {held} Strichen, diese Geste galt also etwas anderem",
  "a text box is written in Helvetica, which cannot draw every character in that text":
    "Ein Textkasten wird in Helvetica geschrieben, und Helvetica kann nicht jedes Zeichen dieses Texts darstellen",
  "that comment has no object to write over": "Dieser Kommentar hat kein Objekt, das sich überschreiben ließe",
  "that comment has no object to remove": "Dieser Kommentar hat kein Objekt, das sich entfernen ließe",
  "the fields are already to be tabbed through in reading order":
    "Die Felder werden schon in Lesereihenfolge mit der Tabulatortaste durchlaufen",
  "that is not a form field placed in this document": "Das ist kein Formularfeld, das in diesem Dokument platziert wurde",
  "that changes nothing about the field": "Das ändert nichts an dem Feld",
  "a mark cannot be moved by ({dx}, {dy})": "Eine Markierung kann nicht um ({dx}, {dy}) verschoben werden",
  "nothing was selected to arrange": "Es wurde nichts zum Anordnen ausgewählt",
  "{count} marks is more than the {MAX_ARRANGED} one arrangement moves":
    "{count} Markierungen sind mehr als die {MAX_ARRANGED}, die ein Anordnen verschiebt",
  "an arrangement is one gesture and this one has no name": "Ein Anordnen ist eine Geste, und diese hat keinen Namen",
  "mark {mark} is placed twice": "Markierung {mark} wird zweimal platziert",
  "mark {mark} is not a picture": "Markierung {mark} ist kein Bild",
  "no field was named to change": "Es wurde kein Feld zum Ändern genannt",
  "{count} fields is more than the {MAX_ARRANGED} one change moves":
    "{count} Felder sind mehr als die {MAX_ARRANGED}, die eine Änderung verschiebt",
  "a change to several fields is one gesture and this one has no name":
    "Eine Änderung mehrerer Felder ist eine Geste, und diese hat keinen Namen",
  "that field has no object to change": "Dieses Feld hat kein Objekt, das sich ändern ließe",
  "that rectangle covers nothing": "Dieses Rechteck umfasst nichts",
  "no pages were named": "Es wurden keine Seiten genannt",
  "this document has {count} pages": (a) => `Dieses Dokument hat ${pages(a.count!)}`,
  "page {page} was named twice": "Seite {page} wurde zweimal genannt",
  "the pages are not in document order": "Die Seiten stehen nicht in der Reihenfolge des Dokuments",
  "`{name}`: another field placed here has this name": "`{name}`: Ein anderes hier platziertes Feld hat diesen Namen",
  "`{name}`: another button of this group has the value `{value}`":
    "`{name}`: Ein anderes Optionsfeld dieser Gruppe hat den Wert `{value}`",
  "save or undo redactions before editing text": "Speichere oder widerrufe die Schwärzungen, bevor du Text bearbeitest",
  "save or undo text edits before redacting": "Speichere oder widerrufe die Textänderungen, bevor du schwärzt",
  "text changed since it was selected": "Der Text hat sich geändert, seit er ausgewählt wurde",
  "text edit history is full": "Der Verlauf der Textänderungen ist voll",
  "text edit history is full; save and reopen before continuing":
    "Der Verlauf der Textänderungen ist voll; speichere und öffne das Dokument erneut, bevor du weitermachst",
  "text no longer belongs to this page": "Der Text gehört nicht mehr zu dieser Seite",
  "text replacement exceeds its limit": "Der Ersatztext ist länger als erlaubt",
  "too many text replacements": "Zu viele Textänderungen",
  "no such page": "Diese Seite gibt es nicht",
  "that page has been deleted": "Diese Seite wurde gelöscht",
  "a page cannot be moved after itself": "Eine Seite kann nicht hinter sich selbst verschoben werden",
  "that crop encloses no area": "Dieser Zuschnitt umschließt keine Fläche",
  "a page must have a width and a height": "Eine Seite braucht eine Breite und eine Höhe",
  "a blank page has nothing to crop to": "Eine leere Seite hat nichts, worauf sich zuschneiden ließe",
  "a blank page has nothing to remove": "Eine leere Seite hat nichts, was sich entfernen ließe",
  "choose at least one page to insert": "Wähle mindestens eine Seite zum Einfügen",
  "that document has {pages} pages, so there is no page {number}": (a) =>
    `Dieses Dokument hat ${pages(a.pages!)}, eine Seite ${a.number} gibt es also nicht`,
  "page {number} was chosen twice --- insert it once, then insert it again":
    "Seite {number} wurde zweimal gewählt – füge sie einmal ein und dann noch einmal",
  "no such inserted pages": "Diese eingefügten Seiten gibt es nicht",
  "that page of the other document is in this one twice, and an edit would appear in both --- delete one of them, then edit the other":
    "Diese Seite des anderen Dokuments ist zweimal in diesem, und eine Änderung erschiene in beiden – lösche eine davon und bearbeite dann die andere",
  "page {number} of that document already has edited text in this one, and inserting it again would show the edit twice --- undo the edit, or insert a different page":
    "Seite {number} dieses Dokuments hat hier schon bearbeiteten Text, und ein erneutes Einfügen würde die Änderung zweimal zeigen – widerrufe die Änderung oder füge eine andere Seite ein",
  "that comment is not on a page inserted from another document":
    "Dieser Kommentar steht nicht auf einer Seite, die aus einem anderen Dokument eingefügt wurde",
  "that page came from another document, and tpdf can only remove content from the pages of the file you opened --- save this document, reopen it, and redact there":
    "Diese Seite stammt aus einem anderen Dokument, und tpdf kann nur aus den Seiten der geöffneten Datei Inhalte entfernen – speichere dieses Dokument, öffne es erneut und schwärze dort",
  "no such mark": "Diese Markierung gibt es nicht",
  "that mark has already been removed": "Diese Markierung wurde schon entfernt",
  "that mark covers nothing": "Diese Markierung umfasst nichts",
  "a {kind:?} mark cannot carry the strokes it was sent with":
    "Eine Markierung der Art {kind} kann die mitgeschickten Striche nicht tragen",
  "a {kind:?} mark cannot carry the stamp name it was sent with":
    "Eine Markierung der Art {kind} kann den mitgeschickten Stempelnamen nicht tragen",
  "a {kind:?} mark cannot carry the form field kind it was sent with":
    "Eine Markierung der Art {kind} kann die mitgeschickte Feldart nicht tragen",
  "a {kind:?} mark cannot answer a comment": "Eine Markierung der Art {kind} kann nicht auf einen Kommentar antworten",
  "delete your reply to that comment first, then the comment":
    "Lösche zuerst deine Antwort auf diesen Kommentar und dann den Kommentar",
  "no such redaction": "Diese Schwärzung gibt es nicht",
  "that redaction has already been removed": "Diese Schwärzung wurde schon entfernt",
  "that region covers nothing": "Dieser Bereich umfasst nichts",
  "that note is {chars} characters, and a note holds at most {most}":
    "Diese Notiz hat {chars} Zeichen, und eine Notiz hat höchstens {most}",
  "no open document {doc}": "Kein geöffnetes Dokument {doc}",

  // commands/edit.rs, taborder.rs
  "a picture is sent as bytes": "Ein Bild wird als Bytes geschickt",
  "the picture could not be read: {e}": "Das Bild konnte nicht gelesen werden: {e}",
  "the picture was not read: {e}": "Das Bild wurde nicht gelesen: {e}",
  "The picture was not saved: {why}": "Das Bild wurde nicht gespeichert: {why}",
  "The region was not copied: {why}": "Der Bereich wurde nicht kopiert: {why}",
  "The region was not copied: the clipboard refused it.":
    "Der Bereich wurde nicht kopiert: Die Zwischenablage hat ihn abgelehnt.",
  "the picture could not be read": "Das Bild konnte nicht gelesen werden",
  "The region was not copied: the main thread did not answer.":
    "Der Bereich wurde nicht kopiert: Der Haupt-Thread hat nicht geantwortet.",
  "The region is copied by the web view on this platform.": "Auf diesem System kopiert die Webansicht den Bereich.",
  "The attachment was not saved: {e}": "Der Anhang wurde nicht gespeichert: {e}",
  "its render was abandoned.": "Seine Darstellung wurde abgebrochen.",
  "the picture was not used: {why}": "Das Bild wurde nicht verwendet: {why}",
  "the page lists no annotations": "Die Seite führt keine Anmerkungen",

  // attachments.rs
  "The document has no attachment {number} (it has {count}).":
    "Das Dokument hat keinen Anhang {number} (es hat {count}).",
  "{name} holds no file to save.": "{name} enthält keine Datei zum Speichern.",
  "{name} could not be decoded within {mib} MiB: {why}":
    "{name} konnte nicht innerhalb von {mib} MiB entpackt werden: {why}",
  "The attachment was not saved: that is the document it comes from.":
    "Der Anhang wurde nicht gespeichert: Das ist das Dokument, aus dem er stammt.",
  "The attachment was not saved: {path} names no file.": "Der Anhang wurde nicht gespeichert: {path} nennt keine Datei.",
  "the document is too large to read": "Das Dokument ist zu groß zum Lesen",
  "the worker said {count} bytes and the file holds {held}":
    "Der Worker hat {count} Bytes gemeldet, und die Datei enthält {held}",
  "The attachment was not saved to {path}: {why}": "Der Anhang wurde nicht unter {path} gespeichert: {why}",

  // imagepages.rs: each `it ...` is the reason after "{label} cannot be used:"
  "a resolution of {dpi} DPI is outside {least:.0} to {most:.0}":
    "Eine Auflösung von {dpi} DPI liegt außerhalb von {least} bis {most}",
  "it has no pixels": "Es hat keine Pixel",
  "it is {width} x {height} pixels, and tpdf takes pictures up to {megapixels} megapixels and {MAX_SIDE} pixels a side":
    "Es hat {width} x {height} Pixel, und tpdf nimmt Bilder bis {megapixels} Megapixel und {MAX_SIDE} Pixel je Seite",
  "it is not a complete JPEG image": "Es ist kein vollständiges JPEG-Bild",
  "it is a JPEG with more than eight bits a sample": "Es ist ein JPEG mit mehr als acht Bit je Abtastwert",
  "it is a lossless or arithmetic-coded JPEG, which PDF readers do not display":
    "Es ist ein verlustfreies oder arithmetisch codiertes JPEG, das PDF-Reader nicht anzeigen",
  "it is a CMYK JPEG, which tpdf does not place yet; save it as RGB":
    "Es ist ein CMYK-JPEG, das tpdf noch nicht platziert; speichere es als RGB",
  "it is a JPEG with {other} colour components": "Es ist ein JPEG mit {other} Farbkomponenten",
  "it does not decode: {e}": "Es lässt sich nicht decodieren: {e}",
  "its header and its picture disagree about its size": "Sein Header und sein Bild nennen verschiedene Größen",
  "its pixels could not be compressed: {e}": "Seine Pixel konnten nicht komprimiert werden: {e}",
  "it is not a complete PNG image": "Es ist kein vollständiges PNG-Bild",
  "it is larger than its header says": "Es ist größer, als sein Header angibt",
  "it could not be read as eight-bit samples": "Es konnte nicht als 8-Bit-Abtastwerte gelesen werden",
  "its palette could not be expanded": "Seine Palette konnte nicht aufgelöst werden",
  "it is not the size its header says": "Es hat nicht die Größe, die sein Header angibt",
  "it is not a PNG or JPEG image": "Es ist kein PNG- oder JPEG-Bild",
  "{chosen} pictures were chosen, and tpdf makes a document from at most {MAX_IMAGES} at once":
    "{chosen} Bilder wurden gewählt, und tpdf macht ein Dokument aus höchstens {MAX_IMAGES} auf einmal",
  "{label} cannot be used: {why}": "{label} kann nicht verwendet werden: {why}",

  // picture.rs
  "not a picture upload: {why}": "Kein Bild-Upload: {why}",
  "too short": "zu kurz",
  "the header length is out of range": "Die Länge des Headers liegt außerhalb des Bereichs",
  "the mark is not a picture": "Die Markierung ist kein Bild",
  "the pixels came twice": "Die Pixel kamen zweimal",
  "the pixel count does not match the size": "Die Zahl der Pixel passt nicht zur Größe",

  // region.rs
  "The picture was not saved: {path} does not end in .png.": "Das Bild wurde nicht gespeichert: {path} endet nicht auf .png.",
  "The picture was not saved: the render did not come back as a PNG.":
    "Das Bild wurde nicht gespeichert: Die Darstellung kam nicht als PNG zurück.",
  "The picture was not saved: {path} names no file.": "Das Bild wurde nicht gespeichert: {path} nennt keine Datei.",
  "The picture was not saved to {path}: {why}": "Das Bild wurde nicht unter {path} gespeichert: {why}",
} satisfies Record<string, Wording>;
