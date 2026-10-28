/** `progressive.rs`: opening a document and rendering its pages. */

import type { Wording } from "./de";

export default {
  "could not load Pdfium from {path}: {e}": "PDFium konnte nicht aus {path} geladen werden: {e}",
  "This document is locked, and needs a password.": "Dieses Dokument ist gesperrt und braucht ein Passwort.",
  "This document uses a security scheme tpdf cannot read.":
    "Dieses Dokument verwendet ein Sicherheitsverfahren, das tpdf nicht lesen kann.",
  "This file is not a PDF, or it is damaged beyond reading.":
    "Diese Datei ist kein PDF oder so beschädigt, dass sie nicht gelesen werden kann.",
  "This file could not be read from disk.": "Diese Datei konnte nicht vom Datenträger gelesen werden.",
  "This document could not be opened, and PDFium did not say why.":
    "Dieses Dokument konnte nicht geöffnet werden, und PDFium hat keinen Grund genannt.",
  "path is not UTF-8: {path}": "Der Pfad ist kein UTF-8: {path}",
  "no such page: {index}": "Diese Seite gibt es nicht: {index}",
  "page {index}: the crop box is not a number": "Seite {index}: der Beschnittrahmen ist keine Zahl",
  "buffer is {have} bytes, need exactly {needed} for {width}x{height}":
    "Der Puffer hat {have} Bytes, für {width}x{height} werden genau {needed} gebraucht",
  "could not create a {width}x{height} bitmap": "Eine Bitmap mit {width}x{height} konnte nicht angelegt werden",
  "a {width}x{height} tile needs {want} bytes": "Eine Kachel mit {width}x{height} braucht {want} Bytes",
} satisfies Record<string, Wording>;
