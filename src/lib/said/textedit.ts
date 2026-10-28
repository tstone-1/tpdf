/** Editing text already on a page, and adding text: `textedit.rs` and
 * `textedit/`, `addedtext.rs`, `textlayer.rs`, `text.rs` and `textview.rs`.
 *
 * Most of these say why a run of text cannot be edited. They name PDF
 * operators, font formats and dictionary keys as they are written in the
 * file, so those stay as tokens. */

import type { Wording } from "./de";

/** The tagged structure's parts, as `keys` in `tagging.rs` names them. */
const CONTEXT = "getaggten Struktur";

export default {
  // textedit.rs
  "invalid text resources": "Die Textressourcen sind ungültig",
  "text resource inheritance exceeds its limit": "Die Vererbung der Textressourcen überschreitet ihre Grenze",
  "unsupported optional content": "Nicht unterstützter optionaler Inhalt",
  "unsupported placed-artwork marked content": "Nicht unterstützter markierter Inhalt einer platzierten Grafik",
  "text editing requires a standard font or a supported embedded font":
    "Zum Bearbeiten braucht der Text eine Standardschrift oder eine unterstützte eingebettete Schrift",
  "custom font metrics or character mappings are not editable yet":
    "Eigene Schriftmetriken oder Zeichenzuordnungen lassen sich noch nicht bearbeiten",
  "unsupported standard font encoding": "Nicht unterstützte Kodierung der Standardschrift",
  "invalid text position or size": "Ungültige Textposition oder -größe",
  "text position or size exceeds its limit": "Textposition oder -größe überschreitet ihre Grenze",
  "text editing currently supports printable Latin-1, WinAnsi punctuation and minus only":
    "Die Textbearbeitung unterstützt derzeit nur druckbare Latin-1-Zeichen, WinAnsi-Satzzeichen und das Minuszeichen",
  "text editing currently supports printable Latin-1 only":
    "Die Textbearbeitung unterstützt derzeit nur druckbare Latin-1-Zeichen",
  "too many page content streams": "Die Seite hat zu viele Inhaltsstreams",
  "page content exceeds its limit": "Der Seiteninhalt überschreitet seine Grenze",
  "text position exceeds its limit": "Die Textposition überschreitet ihre Grenze",
  "composed text transform exceeds its limit": "Die zusammengesetzte Texttransformation überschreitet ihre Grenze",
  "unsupported kerning array shape or size": "Form oder Größe des Kerning-Arrays wird nicht unterstützt",
  "kerning array text exceeds its limit": "Der Text des Kerning-Arrays überschreitet seine Grenze",
  "kerning position exceeds its limit": "Die Kerning-Position überschreitet ihre Grenze",
  "text page is not in this document": "Die Seite des Textes gehört nicht zu diesem Dokument",
  "a repeated page object is not editable": "Ein mehrfach verwendetes Seitenobjekt lässt sich nicht bearbeiten",
  "text operator count exceeds its limit": "Die Anzahl der Textoperatoren überschreitet ihre Grenze",
  "unterminated ActualText marked content": "Nicht abgeschlossener markierter ActualText-Inhalt",
  "unterminated placed-artwork marked content": "Nicht abgeschlossener markierter Inhalt einer platzierten Grafik",
  "unterminated Fill & Sign marked content": "Nicht abgeschlossener markierter Fill-&-Sign-Inhalt",
  "unterminated text block": "Nicht abgeschlossener Textblock",
  "unterminated graphics-state save": "Nicht abgeschlossene Sicherung des Grafikzustands",
  "page contains only read-only text": "Die Seite enthält nur schreibgeschützten Text",
  "page annotations are not an array": "Die Anmerkungen der Seite sind kein Array",
  "page annotation count exceeds its limit": "Die Anzahl der Anmerkungen der Seite überschreitet ihre Grenze",
  "page annotation is not a dictionary": "Eine Anmerkung der Seite ist kein Dictionary",
  "page annotation has no rectangle": "Eine Anmerkung der Seite hat kein Rechteck",
  "page annotation rectangle is not an array": "Das Rechteck einer Anmerkung der Seite ist kein Array",
  "page annotation rectangle is not four numbers": "Das Rechteck einer Anmerkung der Seite besteht nicht aus vier Zahlen",
  "page annotation rectangle exceeds its limit": "Das Rechteck einer Anmerkung der Seite überschreitet seine Grenze",
  "cannot preserve following text at PDF number precision":
    "Der folgende Text lässt sich mit der Zahlengenauigkeit von PDF nicht erhalten",
  "duplicate text replacement": "Doppelte Textersetzung",
  "invalid or oversized replacement text": "Der Ersatztext ist ungültig oder zu groß",
  "text replacement is unchanged": "Die Textersetzung ändert nichts",
  "missing text page": "Die Seite des Textes fehlt",
  "ActualText editing currently requires a single line": "ActualText lässt sich derzeit nur einzeilig bearbeiten",
  "missing link annotation": "Die Link-Anmerkung fehlt",
  "text run no longer exists": "Diesen Textabschnitt gibt es nicht mehr",
  "text changed since this run was inspected": "Der Text hat sich geändert, seit dieser Abschnitt geprüft wurde",
  "missing text font": "Die Schrift des Textes fehlt",
  "missing text spacing": "Die Abstände des Textes fehlen",
  "missing text ink bounds": "Die Farbgrenzen des Textes fehlen",
  "replacement would exceed the original text advance": "Der Ersatz wäre breiter als der ursprüngliche Text",
  "replacement ink would exceed the original text bounds":
    "Der Ersatz würde über die Grenzen des ursprünglichen Textes hinaus zeichnen",
  "Text edits would exceed the page's 32-font limit":
    "Die Textänderungen würden die Grenze von 32 Schriften pro Seite überschreiten",

  // addedtext.rs
  "the text to add is empty": "Der hinzuzufügende Text ist leer",
  "the text to add is longer than {MAX_CHARS} characters":
    "Der hinzuzufügende Text ist länger als {MAX_CHARS} Zeichen",
  "the text {text:?} has a control character in it": "Der Text {text} enthält ein Steuerzeichen",
  "the text {text:?} has characters tpdf has no font for: it writes Latin, Greek, Cyrillic, Chinese, Japanese and Korean, and not scripts that join or reorder their letters, such as Arabic, Hebrew, Hindi or Thai":
    "Der Text {text} enthält Zeichen, für die tpdf keine Schrift hat: tpdf schreibt Lateinisch, Griechisch, Kyrillisch, Chinesisch, Japanisch und Koreanisch, aber keine Schriften, die ihre Buchstaben verbinden oder umordnen, etwa Arabisch, Hebräisch, Hindi oder Thai",
  "the text {text:?} opens a field with `{{` and does not close it":
    "Der Text {text} öffnet ein Feld mit `{` und schließt es nicht",
  "the text {text:?} names the field {field}, and the fields are {page} and {pages}":
    "Der Text {text} nennt das Feld {field}, die Felder heißen aber {page} und {pages}",
  "a font size of {size} points is outside {low} to {high}":
    "Eine Schriftgröße von {size} Punkt liegt außerhalb von {low} bis {high}",
  "a colour is red, green and blue, each from 0 to 1": "Eine Farbe besteht aus Rot, Grün und Blau, jeweils von 0 bis 1",
  "the opacity is more than 0 and at most 1": "Die Deckkraft ist größer als 0 und höchstens 1",
  "the angle is in degrees, from -360 to 360": "Der Winkel wird in Grad angegeben, von -360 bis 360",
  "the margin is from 0 to {MAX_MARGIN} points": "Der Rand liegt zwischen 0 und {MAX_MARGIN} Punkt",
  "the first number is at most {MAX_FIRST}": "Die erste Zahl ist höchstens {MAX_FIRST}",
  "{count} lines of text to add, and one write takes {MAX_LINES}":
    "{count} Textzeilen sollen hinzukommen, ein Schreibvorgang nimmt aber {MAX_LINES}",
  "the text is to go on page {page}, and the document has {count}":
    "Der Text soll auf Seite {page}, das Dokument hat aber {count}",
  "pages {first} and {second} are one page in this file, listed twice, so a line cannot be written on each":
    "Die Seiten {first} und {second} sind in dieser Datei eine Seite, die zweimal aufgeführt ist, daher kann nicht auf jede eine Zeile geschrieben werden",
  "no font was made for a line of text": "Für eine Textzeile wurde keine Schrift angelegt",

  // textlayer.rs
  "the text layer names page {page}, and the document has {count}":
    "Die Textebene nennt Seite {page}, das Dokument hat aber {count}",
  "the text layer names page {page} twice": "Die Textebene nennt Seite {page} zweimal",
  "the text layer for page {page} has {count} words, over the limit of {MAX_WORDS}":
    "Die Textebene für Seite {page} hat {count} Wörter, mehr als die Grenze von {MAX_WORDS}",
  "a word in the text layer is {count} characters long, over the limit of {MAX_UNITS}":
    "Ein Wort der Textebene ist {count} Zeichen lang, mehr als die Grenze von {MAX_UNITS}",

  // text.rs
  "could not load the page's text": "Der Text der Seite konnte nicht geladen werden",

  // textedit/actual.rs
  "unsupported ActualText marked-content sequence": "Nicht unterstützte Folge von markiertem ActualText-Inhalt",
  "read-only ActualText requires validated glyph outlines":
    "Schreibgeschützter ActualText braucht geprüfte Glyphenumrisse",

  // textedit/centred.rs
  'a centred line needs a text layout to stay centred; in an edit plan, name a "font"':
    "Eine zentrierte Zeile braucht ein Textlayout, um zentriert zu bleiben; nenne in einem Bearbeitungsplan eine „font“",

  // textedit/clipping.rs
  "partly clipped text is not editable yet": "Teilweise beschnittener Text lässt sich noch nicht bearbeiten",
  "compound clip coordinates exceed their limit":
    "Die Koordinaten des zusammengesetzten Beschneidungspfads überschreiten ihre Grenze",
  "too many compound clip rectangles": "Zu viele Rechtecke im zusammengesetzten Beschneidungspfad",
  "clip coordinates exceed their limit": "Die Koordinaten des Beschneidungspfads überschreiten ihre Grenze",
  "only complete unpainted rectangular clips are editable":
    "Bearbeiten lässt sich nur mit vollständigen, nicht gemalten rechteckigen Beschneidungspfaden",
  "empty text clipping intersection": "Die Schnittmenge der Beschneidungspfade des Textes ist leer",
  "invalid rectangle paint operands": "Ungültige Operanden zum Malen eines Rechtecks",
  "invalid rectangle operands": "Ungültige Operanden eines Rechtecks",
  "empty rectangle is not editable": "Ein leeres Rechteck lässt sich nicht bearbeiten",
  "rectangle coordinates exceed their limit": "Die Koordinaten des Rechtecks überschreiten ihre Grenze",
  "only complete bounded painted paths are editable":
    "Bearbeiten lässt sich nur mit vollständigen, begrenzten gemalten Pfaden",
  "path coordinates exceed their limit": "Die Koordinaten des Pfads überschreiten ihre Grenze",
  "invalid painted path": "Ungültiger gemalter Pfad",
  "negative line width is not editable": "Eine negative Linienstärke lässt sich nicht bearbeiten",

  // textedit/colors.rs
  "unsupported text rendering intent": "Nicht unterstützter Rendering-Intent des Textes",
  "default colour-space substitution is not editable yet":
    "Ersetzte Standardfarbräume lassen sich noch nicht bearbeiten",
  "unsupported text colour space or ICC profile header":
    "Nicht unterstützter Farbraum des Textes oder ICC-Profilkopf",
  "wrong text colour component count": "Falsche Anzahl von Farbkomponenten des Textes",
  "text colour component is out of range": "Eine Farbkomponente des Textes liegt außerhalb des gültigen Bereichs",

  // textedit/filters.rs
  "external or parameterised content streams are not editable yet":
    "Externe oder parametrisierte Inhaltsstreams lassen sich noch nicht bearbeiten",
  "encoded page content exceeds its limit": "Der kodierte Seiteninhalt überschreitet seine Grenze",
  "unsupported content stream filter": "Nicht unterstützter Filter des Inhaltsstreams",
  "invalid Flate content: {e}": "Ungültiger Flate-Inhalt: {e}",
  "incomplete or oversized Flate content": "Der Flate-Inhalt ist unvollständig oder zu groß",
  "ASCII85 output exceeds its limit": "Die ASCII85-Ausgabe überschreitet ihre Grenze",
  "invalid ASCII85 end marker or partial group": "Ungültige ASCII85-Endmarke oder unvollständige Gruppe",
  "ASCII85 group out of range": "Eine ASCII85-Gruppe liegt außerhalb des gültigen Bereichs",
  "invalid ASCII85 character or zero group": "Ungültiges ASCII85-Zeichen oder ungültige Nullgruppe",
  "ASCII85 content is missing its end marker": "Dem ASCII85-Inhalt fehlt die Endmarke",

  // textedit/fonts.rs
  "invalid Type1 font descriptor": "Ungültiger Schriftdeskriptor einer Type1-Schrift",
  "Type1 font has conflicting embedded font programs":
    "Die Type1-Schrift hat widersprüchliche eingebettete Schriftprogramme",
  "invalid embedded program in Type1 font": "Ungültiges eingebettetes Programm in einer Type1-Schrift",
  "FontFile2 is not supported in Type1 fonts": "FontFile2 wird in Type1-Schriften nicht unterstützt",
  "OpenType programs in Type1 fonts are not editable yet":
    "OpenType-Programme in Type1-Schriften lassen sich noch nicht bearbeiten",
  "unsupported embedded program subtype in Type1 font":
    "Nicht unterstützter Untertyp des eingebetteten Programms in einer Type1-Schrift",
  "This font shows spaces as gaps between words; use single spaces between words":
    "Diese Schrift zeigt Leerzeichen als Lücken zwischen Wörtern; setze nur einfache Leerzeichen zwischen Wörter",
  "embedded font does not permit this editable use":
    "Die eingebettete Schrift erlaubt diese Verwendung zum Bearbeiten nicht",
  "invalid or oversized two-byte text": "Der Zwei-Byte-Text ist ungültig oder zu groß",
  "text contains an unmapped font code": "Der Text enthält einen Schriftcode ohne Zuordnung",
  "expanded two-byte text exceeds its limit": "Der erweiterte Zwei-Byte-Text überschreitet seine Grenze",
  "mapped text exceeds its limit": "Der zugeordnete Text überschreitet seine Grenze",
  "expanded text exceeds its limit": "Der erweiterte Text überschreitet seine Grenze",
  "the font has no validated glyph for this character": "Die Schrift hat für dieses Zeichen keine geprüfte Glyphe",
  "character spacing exceeds a quarter of the font size":
    "Der Zeichenabstand ist größer als ein Viertel der Schriftgröße",
  "word spacing exceeds editable limits": "Der Wortabstand überschreitet die Grenzen für das Bearbeiten",
  "backtracking character spacing is not editable yet":
    "Ein rückwärts laufender Zeichenabstand lässt sich noch nicht bearbeiten",
  "spaced text advance exceeds its limit": "Die Breite des gesperrten Textes überschreitet ihre Grenze",
  "unsupported unembedded font": "Nicht unterstützte, nicht eingebettete Schrift",
  "unsupported embedded TrueType font or character mapping":
    "Nicht unterstützte eingebettete TrueType-Schrift oder Zeichenzuordnung",
  "embedded font widths disagree with its glyph metrics":
    "Die Breiten der eingebetteten Schrift widersprechen ihren Glyphenmetriken",
  "unsupported embedded TrueType program": "Nicht unterstütztes eingebettetes TrueType-Programm",
  "unsupported installed OpenType program": "Nicht unterstütztes installiertes OpenType-Programm",

  // textedit/forms.rs
  "unsupported preserved Form XObject": "Nicht unterstütztes beibehaltenes Form-XObject",

  // textedit/graphics.rs
  "unsupported line stroke state": "Nicht unterstützter Linienzustand",
  "unsupported rendering tolerance": "Nicht unterstützte Rendering-Toleranz",
  "unsupported external text graphics state": "Nicht unterstützter externer Grafikzustand des Textes",

  // textedit/images.rs
  "unsupported image on an editable page": "Nicht unterstütztes Bild auf einer bearbeitbaren Seite",
  "decoded images exceed the editable page budget":
    "Die dekodierten Bilder überschreiten das Budget einer bearbeitbaren Seite",
  "image samples do not match dimensions and colour components":
    "Die Bilddaten passen nicht zu Abmessungen und Farbkomponenten",

  // textedit/inspect.rs
  "text graphics-state stack exceeds its limit": "Der Stapel der Grafikzustände des Textes überschreitet seine Grenze",
  "unmatched graphics-state restore": "Wiederherstellung eines Grafikzustands ohne passende Sicherung",
  "non-diagonal clips are not editable yet": "Nicht achsenparallele Beschneidungspfade lassen sich noch nicht bearbeiten",
  "too many compound clipping intersections": "Zu viele zusammengesetzte Schnittmengen von Beschneidungspfaden",
  "unsupported text size": "Nicht unterstützte Textgröße",
  "nested ActualText marked content is not editable yet":
    "Verschachtelter markierter ActualText-Inhalt lässt sich noch nicht bearbeiten",
  "marked content inside optional content is not editable yet":
    "Markierter Inhalt in optionalem Inhalt lässt sich noch nicht bearbeiten",
  "marked content inside placed artwork is not editable yet":
    "Markierter Inhalt in einer platzierten Grafik lässt sich noch nicht bearbeiten",
  "marked content inside a Fill & Sign addition is not editable yet":
    "Markierter Inhalt in einer Fill-&-Sign-Ergänzung lässt sich noch nicht bearbeiten",
  "too many ActualText spans": "Zu viele ActualText-Abschnitte",
  "too many external text graphics states": "Zu viele externe Grafikzustände des Textes",
  "too many text colour spaces": "Zu viele Farbräume des Textes",
  "too many fonts on an editable page": "Zu viele Schriften auf einer bearbeitbaren Seite",
  "too many images on an editable page": "Zu viele Bilder auf einer bearbeitbaren Seite",
  "text has no explicit font": "Der Text hat keine ausdrücklich gesetzte Schrift",
  "text has no preceding position": "Der Text hat keine vorangehende Position",
  "continued text advance exceeds its limit": "Die Breite des fortgesetzten Textes überschreitet ihre Grenze",
  "text bounds exceed the display range": "Die Grenzen des Textes überschreiten den Anzeigebereich",
  "read-only text requires validated glyph outlines": "Schreibgeschützter Text braucht geprüfte Glyphenumrisse",
  "clipped text requires validated embedded glyph outlines":
    "Beschnittener Text braucht geprüfte eingebettete Glyphenumrisse",

  // textedit/layout.rs
  "missing text line origin": "Der Ursprung der Textzeile fehlt",
  "Cannot preserve the following line origin precisely":
    "Der Ursprung der folgenden Zeile lässt sich nicht genau erhalten",
  "The table states its bounds, and this text would leave them. Reduce the box or font size to keep the text inside.":
    "Die Tabelle legt ihre Grenzen fest, und dieser Text würde sie verlassen. Verkleinere den Kasten oder die Schrift, damit der Text darin bleibt.",
  "There is no room for more text on this line: other text follows it. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Danach folgt anderer Text. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: the text after it cannot be moved. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Der Text danach lässt sich nicht verschieben. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: a picture or a drawing follows it. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Danach folgt ein Bild oder eine Zeichnung. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: it reaches the edge of the page. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Sie reicht bis an den Seitenrand. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: the document clips the space after it. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Das Dokument beschneidet den Raum dahinter. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: the table states its bounds, and the text would leave them. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Die Tabelle legt ihre Grenzen fest, und der Text würde sie verlassen. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line: it reaches the next column. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text: Sie reicht bis an die nächste Spalte. Kürze den Text oder verkleinere die Schrift.",
  "Cannot move the following text at PDF number precision":
    "Der folgende Text lässt sich mit der Zahlengenauigkeit von PDF nicht verschieben",
  "missing text layout context": "Der Layoutkontext des Textes fehlt",
  "invalid text patch": "Ungültige Textänderung",
  "Text exceeds 128 lines": "Der Text hat mehr als 128 Zeilen",
  "A centred line cannot be wrapped yet. Keep the text on one line.":
    "Eine zentrierte Zeile lässt sich noch nicht umbrechen. Halte den Text in einer Zeile.",
  "There is no room to keep this line centred. Shorten the text or reduce the font size.":
    "Es ist kein Platz, um diese Zeile zentriert zu halten. Kürze den Text oder verkleinere die Schrift.",
  "This text's font does not permit editing. Choose automatic fallback or a Noto font.":
    "Die Schrift dieses Textes erlaubt kein Bearbeiten. Wähle die automatische Ersatzschrift oder eine Noto-Schrift.",
  "missing text layout": "Das Textlayout fehlt",
  "Use a font size from 1 to 512 pt and a box from 0.1 to 14400 pt":
    "Verwende eine Schriftgröße von 1 bis 512 pt und einen Kasten von 0,1 bis 14400 pt",
  "Text transformation exceeds its limit": "Die Texttransformation überschreitet ihre Grenze",
  "Original font": "Originalschrift",
  "too many replacement font resources": "Zu viele Schriftressourcen für den Ersatz",
  "{font} (the document's font does not permit editing)": "{font} (die Schrift des Dokuments erlaubt kein Bearbeiten)",
  "Text exceeds the box width. Widen the box, reduce the font size, or enable wrapping.":
    "Der Text ist breiter als der Kasten. Verbreitere den Kasten, verkleinere die Schrift oder schalte den Umbruch ein.",
  "Text ink exceeds the box. Increase its width or choose another font.":
    "Der Text zeichnet über den Kasten hinaus. Verbreitere ihn oder wähle eine andere Schrift.",
  "The editing box extends beyond the page": "Der Bearbeitungskasten reicht über die Seite hinaus",
  "Text exceeds the box height. Enlarge the box or reduce the font size.":
    "Der Text ist höher als der Kasten. Vergrößere den Kasten oder verkleinere die Schrift.",
  "The document clips this area. Reduce the box or font size to keep the text visible.":
    "Das Dokument beschneidet diesen Bereich. Verkleinere den Kasten oder die Schrift, damit der Text sichtbar bleibt.",
  "Text would overlap another line. Reduce the font size or change the box dimensions.":
    "Der Text würde eine andere Zeile überdecken. Verkleinere die Schrift oder ändere die Maße des Kastens.",
  "Cannot preserve the following text position precisely":
    "Die Position des folgenden Textes lässt sich nicht genau erhalten",

  // textedit/patterns.rs
  "unsupported shading pattern on editable page": "Nicht unterstütztes Verlaufsmuster auf einer bearbeitbaren Seite",
  "too many shading patterns": "Zu viele Verlaufsmuster",
  "painting with an unselected shading pattern": "Gemalt wird mit einem nicht ausgewählten Verlaufsmuster",

  // textedit/refusal.rs
  "inline {name} marked content is not editable yet": "Markierter {name}-Inhalt im Text lässt sich noch nicht bearbeiten",
  "nested text blocks are not editable": "Verschachtelte Textblöcke lassen sich nicht bearbeiten",
  "text block ends without a matching beginning": "Ein Textblock endet ohne passenden Anfang",
  "text appears outside a text block": "Text steht außerhalb eines Textblocks",
  "text has no explicit initial position": "Der Text hat keine ausdrücklich gesetzte Anfangsposition",
  "text-show operands have an unsupported shape": "Die Operanden der Textausgabe haben eine nicht unterstützte Form",
  "text positioning shorthand {name} is not editable yet":
    "Die Kurzform {name} zur Textpositionierung lässt sich noch nicht bearbeiten",
  "text rise must be zero for editing": "Zum Bearbeiten muss die Grundlinienverschiebung null sein",
  "text horizontal scaling must be 100 percent for editing":
    "Zum Bearbeiten muss die horizontale Skalierung des Textes 100 Prozent betragen",
  "text that adds to the clipping path is not editable":
    "Text, der zum Beschneidungspfad beiträgt, lässt sich nicht bearbeiten",
  "graphics operation {name} inside a text block is not editable yet":
    "Die Grafikoperation {name} in einem Textblock lässt sich noch nicht bearbeiten",
  "text setup operation {name} outside a text block is not editable yet":
    "Die Texteinstellung {name} außerhalb eines Textblocks lässt sich noch nicht bearbeiten",
  "unsupported operands for {name} (count: {count})": "Nicht unterstützte Operanden für {name} (Anzahl: {count})",
  "path operation {name} is outside a supported complete path":
    "Die Pfadoperation {name} steht außerhalb eines unterstützten vollständigen Pfads",
  "shading patterns are not editable yet": "Verlaufsmuster lassen sich noch nicht bearbeiten",
  "inline images are not editable yet": "Eingebettete Inline-Bilder lassen sich noch nicht bearbeiten",
  "content operation {name} is not editable yet": "Die Inhaltsoperation {name} lässt sich noch nicht bearbeiten",
  "unrecognized content operator is not editable yet":
    "Ein unbekannter Inhaltsoperator lässt sich noch nicht bearbeiten",

  // textedit/spacers.rs
  "unsupported inline spacing sequence": "Nicht unterstützte Abstandsfolge im Text",
  "inline BDC marked content is not editable yet": "Markierter BDC-Inhalt im Text lässt sich noch nicht bearbeiten",

  // textedit/streams.rs
  "cannot preserve text stream tokens": "Die Tokens des Textstreams lassen sich nicht erhalten",
  "edited page exceeds the text operator limit": "Die bearbeitete Seite überschreitet die Grenze für Textoperatoren",
  "missing operator start": "Der Anfang eines Operators fehlt",
  "text operator boundaries disagree": "Die Grenzen der Textoperatoren widersprechen sich",
  "untouched text operator changed": "Ein unberührter Textoperator hat sich geändert",
  "text page content exceeds its limit": "Der Seiteninhalt des Textes überschreitet seine Grenze",

  // textedit/tagging.rs
  "unsupported or inconsistent tagged text structure": "Nicht unterstützte oder widersprüchliche Tag-Struktur des Textes",
  "unsupported {feature} metadata in tagged {context}": (a) =>
    a.feature === "unrecognized"
      ? `Nicht erkannte Metadaten in der ${CONTEXT} (${a.context})`
      : `Nicht unterstützte Metadaten ${a.feature} in der ${CONTEXT} (${a.context})`,
  "parent tree": "Elternbaum",
  "tagged parent tree must contain one entry per page": "Der getaggte Elternbaum muss einen Eintrag pro Seite enthalten",
  element: "Element",
  "unsupported NS metadata in tagged element": "Nicht unterstützte NS-Metadaten in einem getaggten Element",
  "field attributes": "Feldattribute",
  "bounded attributes": "Begrenzungsattribute",
  "list attributes": "Listenattribute",
  "layout attributes": "Layoutattribute",
  "unsupported BBox metadata in tagged layout attributes":
    "Nicht unterstützte BBox-Metadaten in getaggten Layoutattributen",
  "object reference": "Objektverweis",
  "tagged annotation and parent tree disagree on ownership":
    "Getaggte Anmerkung und Elternbaum widersprechen sich in der Zugehörigkeit",
  "structure root": "Strukturwurzel",
  namespace: "Namensraum",
  "unsupported tagged namespace": "Nicht unterstützter getaggter Namensraum",
  "tagged RoleMap contains unsupported or conflicting roles":
    "Die getaggte RoleMap enthält nicht unterstützte oder widersprüchliche Rollen",
  "tagged parent content is empty or exceeds its limit":
    "Der getaggte Elterninhalt ist leer oder überschreitet seine Grenze",
  "tagged element role is not editable yet": "Die Rolle des getaggten Elements lässt sich noch nicht bearbeiten",
  "tagged grouping frontier exceeds its limit": "Die getaggte Gruppierungsgrenze überschreitet ihre Grenze",
  "tagged list frontier exceeds its limit": "Die getaggte Listengrenze überschreitet ihre Grenze",
  "tagged sublist frontier exceeds its limit": "Die getaggte Unterlistengrenze überschreitet ihre Grenze",
  "content reference": "Inhaltsverweis",
  "tagged content and parent tree disagree on ownership":
    "Getaggter Inhalt und Elternbaum widersprechen sich in der Zugehörigkeit",
  "tagged parent tree has annotation entries no element claims":
    "Der getaggte Elternbaum hat Anmerkungseinträge, die zu keinem Element gehören",
  "unsupported artifact properties": "Nicht unterstützte Artefakteigenschaften",
  "marked content has no supported structure tree": "Der markierte Inhalt hat keinen unterstützten Strukturbaum",
  "nested marked content is not editable yet": "Verschachtelter markierter Inhalt lässt sich noch nicht bearbeiten",
  "marked-content properties": "Eigenschaften des markierten Inhalts",
  "marked content repeats or disagrees with its structure tag":
    "Markierter Inhalt wiederholt sich oder widerspricht seinem Struktur-Tag",
  "marked content without MCID must be an Artifact": "Markierter Inhalt ohne MCID muss ein Artifact sein",
  "unmatched marked-content end": "Ende eines markierten Inhalts ohne passenden Anfang",
  "text outside a tagged table cell is not editable":
    "Text außerhalb einer getaggten Tabellenzelle lässt sich nicht bearbeiten",

  // textedit/tagging/tables.rs
  "unsupported IDTree metadata in tagged structure root":
    "Nicht unterstützte IDTree-Metadaten in der getaggten Strukturwurzel",
  "tagged header and IDTree disagree on ownership":
    "Getaggte Kopfzelle und IDTree widersprechen sich in der Zugehörigkeit",
  "table cell layout attributes": "Layoutattribute einer Tabellenzelle",
  "table cell attributes": "Attribute einer Tabellenzelle",
  "tagged cell references an unknown or foreign header":
    "Eine getaggte Zelle verweist auf eine unbekannte oder fremde Kopfzelle",
  "IDTree node": "IDTree-Knoten",

  // textedit/fonts/added.rs
  "no bundled font has a glyph for U+{code:04X}": "Keine mitgelieferte Schrift hat eine Glyphe für U+{code}",
  "a character no bundled font has": "Ein Zeichen, das keine mitgelieferte Schrift hat",
  "a line's character is in no subset made for the write":
    "Ein Zeichen einer Zeile ist in keiner für das Schreiben angelegten Teilschrift",

  // textedit/fonts/cff.rs
  "unsupported embedded CFF font": "Nicht unterstützte eingebettete CFF-Schrift",
  "unsupported CFF glyph name": "Nicht unterstützter CFF-Glyphenname",
  "CFF font widths disagree with its glyph metrics": "Die Breiten der CFF-Schrift widersprechen ihren Glyphenmetriken",
  "ambiguous duplicate CFF glyph encoding": "Mehrdeutige doppelte CFF-Glyphenkodierung",

  // textedit/fonts/composite.rs
  "unsupported composite font or character mapping": "Nicht unterstützte zusammengesetzte Schrift oder Zeichenzuordnung",
  "composite font widths disagree with its glyph metrics":
    "Die Breiten der zusammengesetzten Schrift widersprechen ihren Glyphenmetriken",

  // textedit/fonts/fallback.rs
  "Bundled font: {e}": "Mitgelieferte Schrift: {e}",
  "Bundled font subset: {e}": "Teilschrift der mitgelieferten Schrift: {e}",
  "invalid installed font rights": "Ungültige Rechte der installierten Schrift",
  "missing installed CFF program": "Das installierte CFF-Programm fehlt",
  "unreadable installed CFF glyph": "Eine installierte CFF-Glyphe lässt sich nicht lesen",
  "installed CFF widths disagree with its metrics": "Die Breiten der installierten CFF-Schrift widersprechen ihren Metriken",
  "too many replacement characters": "Zu viele Ersatzzeichen",
  "The selected fallback font does not contain a required character":
    "Der gewählten Ersatzschrift fehlt ein benötigtes Zeichen",
  "missing fallback glyph width": "Die Glyphenbreite der Ersatzschrift fehlt",

  // textedit/fonts/fallback_subset.rs
  "Could not subset the bundled editing font": "Die mitgelieferte Bearbeitungsschrift konnte nicht verkleinert werden",
  "Missing bundled font embedding rights": "Die Einbettungsrechte der mitgelieferten Schrift fehlen",
  "The replacement needs too many font outlines; shorten the text":
    "Der Ersatz braucht zu viele Schriftumrisse; kürze den Text",
  "Missing subset glyph": "Eine Glyphe der Teilschrift fehlt",
  "Invalid bundled font subset": "Ungültige Teilschrift der mitgelieferten Schrift",

  // textedit/fonts/installed.rs
  "the installed {name} could not be read": "Die installierte {name} konnte nicht gelesen werden",
  "the installed {name} has CFF2 outlines, which cannot be embedded":
    "Die installierte {name} hat CFF2-Umrisse, die sich nicht einbetten lassen",
  "the installed {name} is a variable or colour font": "Die installierte {name} ist eine variable oder eine Farbschrift",
  "the installed {name} does not permit editing": "Die installierte {name} erlaubt kein Bearbeiten",
  "the installed {name} differs from the document's copy": "Die installierte {name} weicht von der Kopie im Dokument ab",
  "the installed {name} lacks a character": "Der installierten {name} fehlt ein Zeichen",
  "the installed {name} draws two of the characters with one glyph":
    "Die installierte {name} zeichnet zwei der Zeichen mit einer Glyphe",
  "{font} (installed)": "{font} (installiert)",

  // textedit/fonts/mapping.rs
  "unsupported or ambiguous Unicode character map": "Nicht unterstützte oder mehrdeutige Unicode-Zeichenzuordnung",
  "inherited character maps are not editable yet": "Geerbte Zeichenzuordnungen lassen sich noch nicht bearbeiten",
  "unsupported or ambiguous character map": "Nicht unterstützte oder mehrdeutige Zeichenzuordnung",
  "unsupported or ambiguous single-byte character map":
    "Nicht unterstützte oder mehrdeutige Ein-Byte-Zeichenzuordnung",
  "named TrueType ToUnicode disagrees with its ASCII encoding":
    "Das ToUnicode der benannten TrueType-Schrift widerspricht ihrer ASCII-Kodierung",
  "unsupported or ambiguous two-byte character map": "Nicht unterstützte oder mehrdeutige Zwei-Byte-Zeichenzuordnung",

  // textedit/fonts/outlines.rs
  "invalid CFF glyph outline": "Ungültiger CFF-Glyphenumriss",

  // textedit/fonts/type1.rs
  "unsupported embedded Type 1 font": "Nicht unterstützte eingebettete Type-1-Schrift",
  "unsupported or ambiguous Type 1 encoding": "Nicht unterstützte oder mehrdeutige Type-1-Kodierung",
  "ambiguous duplicate Type 1 glyph encoding": "Mehrdeutige doppelte Type-1-Glyphenkodierung",

  // textedit/fonts/type3.rs
  "unsupported or inconsistent Type3 outline font": "Nicht unterstützte oder widersprüchliche Type3-Umrissschrift",
  "invalid Type3 glyph header": "Ungültiger Kopf einer Type3-Glyphe",
  "Type3 glyph must start with d1": "Eine Type3-Glyphe muss mit d1 beginnen",
  "cannot parse Type3 glyph program": "Das Programm einer Type3-Glyphe lässt sich nicht lesen",
  "unsupported Type3 glyph stream dictionary": "Nicht unterstütztes Stream-Dictionary einer Type3-Glyphe",
  "Type3 outline operations exceed their limit": "Die Umrissoperationen der Type3-Schrift überschreiten ihre Grenze",
  "invalid Type3 d1 metrics": "Ungültige d1-Metriken einer Type3-Glyphe",
  "Type3 glyph uses unsupported painting operations": "Eine Type3-Glyphe malt mit nicht unterstützten Operationen",
  "unsupported Type3 font matrix": "Nicht unterstützte Schriftmatrix einer Type3-Schrift",
  "Type3 width disagrees with glyph program": "Die Type3-Breite widerspricht dem Glyphenprogramm",
  "Type3 glyph bounds exceed editable limits": "Die Grenzen einer Type3-Glyphe überschreiten die Grenzen für das Bearbeiten",

  // textedit/fonts/unicode.rs
  "missing composite glyph mapping": "Die Glyphenzuordnung der zusammengesetzten Schrift fehlt",
  "invalid composite glyph index": "Ungültiger Glyphenindex der zusammengesetzten Schrift",
  "missing glyph advance": "Die Glyphenbreite fehlt",
  "invalid replacement text": "Ungültiger Ersatztext",
  "the font has several glyphs for this character and cannot choose one":
    "Die Schrift hat für dieses Zeichen mehrere Glyphen und kann keine davon wählen",
  "invalid single-byte glyph code": "Ungültiger Ein-Byte-Glyphencode",
  "invalid or oversized encoded text": "Der kodierte Text ist ungültig oder zu groß",
  "unmapped font code": "Schriftcode ohne Zuordnung",
  "text spacing exceeds editable limits": "Die Textabstände überschreiten die Grenzen für das Bearbeiten",

  // textedit/fonts/cff/
  "unsupported CID-keyed CFF program": "Nicht unterstütztes CID-basiertes CFF-Programm",
  "unsupported CFF built-in encoding": "Nicht unterstützte eingebaute CFF-Kodierung",
  "unsupported or ambiguous CFF encoding": "Nicht unterstützte oder mehrdeutige CFF-Kodierung",
  "CFF ToUnicode disagrees with its glyph encoding": "Das CFF-ToUnicode widerspricht seiner Glyphenkodierung",
  "unsupported CFF program metadata": "Nicht unterstützte Metadaten des CFF-Programms",
  "unsupported CFF program for an installed font": "Nicht unterstütztes CFF-Programm für eine installierte Schrift",

  // textedit/fonts/type1/program.rs
  "unsupported embedded Type 1 font program": "Nicht unterstütztes eingebettetes Type-1-Schriftprogramm",
  "hexadecimal Type 1 private data is not editable yet":
    "Hexadezimale private Type-1-Daten lassen sich noch nicht bearbeiten",

  // textedit/images/
  "incomplete or unsupported JPEG framing": "Unvollständiger oder nicht unterstützter JPEG-Aufbau",
  "too many JPEG scans": "Zu viele JPEG-Scans",
  "incomplete or unsupported JPEG image": "Unvollständiges oder nicht unterstütztes JPEG-Bild",
  "JPEG dimensions or components disagree with the image dictionary":
    "Abmessungen oder Komponenten des JPEG widersprechen dem Bild-Dictionary",
  "unsupported stencil mask on an editable page": "Nicht unterstützte Schablonenmaske auf einer bearbeitbaren Seite",
  "incomplete or unsupported CCITT image": "Unvollständiges oder nicht unterstütztes CCITT-Bild",

  // textedit/layout/wrap.rs
  "There is no room for more text on this line, and this paragraph cannot wrap: its lines would move onto what is below it. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text, und dieser Absatz kann nicht umbrechen: Seine Zeilen würden auf das rücken, was darunter steht. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line, and this paragraph cannot wrap: part of it below cannot be moved. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text, und dieser Absatz kann nicht umbrechen: Ein Teil darunter lässt sich nicht verschieben. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line, and this paragraph cannot wrap: its lines would move out of line with the text beside them. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text, und dieser Absatz kann nicht umbrechen: Seine Zeilen würden aus der Flucht mit dem Text daneben geraten. Kürze den Text oder verkleinere die Schrift.",
  "There is no room for more text on this line, and this paragraph cannot wrap: a drawing or an annotation is placed over the lines that would move. Shorten the text or reduce the font size.":
    "In dieser Zeile ist kein Platz für mehr Text, und dieser Absatz kann nicht umbrechen: Über den Zeilen, die sich verschieben würden, liegt eine Zeichnung oder eine Anmerkung. Kürze den Text oder verkleinere die Schrift.",
  "One edit in this paragraph wraps onto a new line and moves text that another pending edit changes. Save first, then make the second edit again.":
    "Eine Änderung in diesem Absatz bricht in eine neue Zeile um und verschiebt Text, den eine andere offene Änderung ändert. Speichere zuerst und nimm dann die zweite Änderung erneut vor.",
  "painted path no longer exists": "Diesen gemalten Pfad gibt es nicht mehr",
  "painted path cannot be moved": "Der gemalte Pfad lässt sich nicht verschieben",
} satisfies Record<string, Wording>;
