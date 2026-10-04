// A form made by tpdf, read and answered by PDFKit: the engine Preview reads
// and saves forms with, and one that shares no code with tpdf.
//
// Run through scripts/made_form_check.py, which makes the two files this
// reads (made.pdf, made-filled.pdf) and reads back the one this writes
// (pdfkit-filled.pdf). Alone: swift scripts/made_form_pdfkit.swift <directory>
import PDFKit
import AppKit

let args = CommandLine.arguments
guard args.count >= 2 else {
    print("[FAIL] usage: made_form_pdfkit.swift <directory>")
    exit(2)
}
let directory = URL(fileURLWithPath: args[1], isDirectory: true)
var failed = 0
func check(_ name: String, _ ok: Bool, _ detail: String = "") {
    print("\(ok ? "[OK]  " : "[FAIL]") \(name)\(detail.isEmpty ? "" : "  " + detail)")
    if !ok { failed += 1 }
}
func widgets(_ file: String) -> [PDFAnnotation] {
    guard let document = PDFDocument(url: directory.appendingPathComponent(file)),
          let page = document.page(at: 0) else {
        check("PDFKit opens \(file)", false)
        exit(1)
    }
    return page.annotations.filter { $0.type == "Widget" }
}
func tooltip(_ widget: PDFAnnotation) -> String {
    widget.value(forAnnotationKey: PDFAnnotationKey(rawValue: "/TU")) as? String ?? ""
}

// The form as it was made.
let made = widgets("made.pdf")
let names = made.map { $0.fieldName ?? "" }
check("the page lists its fields in reading order",
      names == ["Name", "Notes", "Agree", "Colour", "Pay", "Pay", "Pay"], names.joined(separator: ", "))
func one(_ list: [PDFAnnotation], _ name: String) -> PDFAnnotation {
    list.first { $0.fieldName == name }!
}
let name = one(made, "Name")
check("a text field is a text field, with its limit, alignment and tooltip",
      name.widgetFieldType == .text && !name.isMultiline && name.maximumLength == 30
        && name.alignment == .right && tooltip(name) == "Your full name" && !name.isReadOnly,
      "max \(name.maximumLength), alignment \(name.alignment.rawValue), tooltip \(tooltip(name))")
let notes = one(made, "Notes")
check("a field of several lines is one", notes.widgetFieldType == .text && notes.isMultiline)
let agree = one(made, "Agree")
check("a checkbox is a checkbox, not ticked",
      agree.widgetFieldType == .button && agree.widgetControlType == .checkBoxControl
        && agree.buttonWidgetState == .offState)
let colour = one(made, "Colour")
check("a dropdown is a list of its choices, centred, with none chosen",
      colour.widgetFieldType == .choice && !colour.isListChoice && colour.choices == ["Red", "Green", "Blue"]
        && colour.alignment == .center && (colour.widgetStringValue ?? "").isEmpty,
      "\(colour.choices ?? [])")
let pay = made.filter { $0.fieldName == "Pay" }
check("three radio buttons are one group, each with its value, none chosen",
      pay.count == 3 && pay.allSatisfy { $0.widgetFieldType == .button && $0.widgetControlType == .radioButtonControl
        && $0.buttonWidgetState == .offState }
        && pay.map { $0.buttonWidgetStateString } == ["Card", "Cash", "Bank transfer"],
      pay.map { $0.buttonWidgetStateString }.joined(separator: ", "))

// The same form answered by tpdf.
let filled = widgets("made-filled.pdf")
check("PDFKit reads the text tpdf wrote",
      one(filled, "Name").widgetStringValue == "Ada" && one(filled, "Notes").widgetStringValue == "one\ntwo")
check("and the box it ticked", one(filled, "Agree").buttonWidgetState == .onState)
check("and the choice it made", one(filled, "Colour").widgetStringValue == "Green")
let chosen = filled.filter { $0.fieldName == "Pay" }.map { ($0.buttonWidgetStateString, $0.buttonWidgetState) }
check("and the one radio button it chose",
      chosen.map { $0.1 == .onState } == [false, true, false] && chosen[1].0 == "Cash",
      chosen.map { "\($0.0)=\($0.1.rawValue)" }.joined(separator: ", "))

// The form answered by PDFKit, for tpdf to read back.
guard let document = PDFDocument(url: directory.appendingPathComponent("made.pdf")) else { exit(1) }
for widget in document.page(at: 0)!.annotations {
    switch widget.fieldName ?? "" {
    case "Name": widget.widgetStringValue = "Grace"
    case "Notes": widget.widgetStringValue = "first\nsecond"
    case "Agree": widget.buttonWidgetState = .onState
    case "Colour": widget.widgetStringValue = "Blue"
    case "Pay": if widget.buttonWidgetStateString == "Bank transfer" { widget.buttonWidgetState = .onState }
    default: break
    }
}
check("PDFKit answers every field and saves the form",
      document.write(to: directory.appendingPathComponent("pdfkit-filled.pdf")))

// Both pages as PDFKit draws them, for a person to look at.
for file in ["made.pdf", "made-filled.pdf"] {
    if let page = PDFDocument(url: directory.appendingPathComponent(file))?.page(at: 0),
       let tiff = page.thumbnail(of: NSSize(width: 900, height: 1200), for: .mediaBox).tiffRepresentation,
       let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) {
        try? png.write(to: directory.appendingPathComponent(file + ".png"))
    }
}
print(failed == 0 ? "[OK] PDFKit reads the form tpdf made and the answers tpdf wrote" : "[FAIL] \(failed) checks failed")
exit(failed == 0 ? 0 : 1)
