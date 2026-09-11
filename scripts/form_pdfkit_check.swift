// Independent macOS reader check for the synthetic AcroForm round trip.
// Generate with TPDF_FORM_PROBE=<output.pdf> cargo test --lib forms::tests::forms_round_trip_values_and_every_shared_widget_appearance
// Run: swift scripts/form_pdfkit_check.swift <output.pdf> [render-directory]
import PDFKit
import AppKit

let args = CommandLine.arguments
guard args.count >= 2,
      let document = PDFDocument(url: URL(fileURLWithPath: args[1])),
      document.pageCount == 2 else {
    print("[FAIL] PDFKit could not read the two-page form")
    exit(1)
}
var textWidgets = 0
var checkboxes = 0
var valid = true
for index in 0..<document.pageCount {
    let page = document.page(at: index)!
    for annotation in page.annotations {
        if annotation.widgetFieldType == .text {
            valid = valid && annotation.widgetStringValue == "Grüße"
            textWidgets += 1
        }
        if annotation.widgetFieldType == .button {
            valid = valid && annotation.buttonWidgetState == .onState
            checkboxes += 1
        }
    }
    if args.count > 2 {
        let directory = URL(fileURLWithPath: args[2], isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let image = page.thumbnail(of: NSSize(width: 880, height: 600), for: .mediaBox)
        let bitmap = NSBitmapImageRep(data: image.tiffRepresentation!)!
        try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("page-\(index).png"))
    }
}
valid = valid && textWidgets == 2 && checkboxes == 1
print("[\(valid ? "PASS" : "FAIL")] PDFKit: \(textWidgets) text widgets, \(checkboxes) checkbox; saved values and field types")
exit(valid ? 0 : 1)
