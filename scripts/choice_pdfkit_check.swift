// Independent readback of the synthetic mixed-form output.
// Generate with TPDF_CHOICE_UNIQUE_PROBE=<pdf> cargo test --lib choices_and_radio_round_trip_exports_indices_and_appearances
// Run: swift scripts/choice_pdfkit_check.swift <pdf> <render-directory>
// TPDF_CHOICE_PROBE uses duplicate exports; PDFKit ignores their saved index and
// this check deliberately fails its label assertion for that compatibility case.
import PDFKit
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let document = PDFDocument(url: URL(fileURLWithPath: args[1])), document.pageCount == 2 else {
    print("[FAIL] PDFKit could not read the mixed form")
    exit(1)
}
var buttons = 0
var selectedButtons = 0
var choices = 0
var valid = true
let directory = URL(fileURLWithPath: args[2], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
for index in 0..<document.pageCount {
    let page = document.page(at: index)!
    for annotation in page.annotations {
        switch annotation.fieldName ?? "" {
        case "delivery":
            buttons += 1
            let selected = annotation.buttonWidgetState == .onState
            if selected { selectedButtons += 1 }
            valid = valid && selected == (index == 1)
        case "delivery_choice", "items":
            choices += 1
            valid = valid && annotation.widgetFieldType == .choice
            if annotation.fieldName == "delivery_choice" {
                valid = valid && annotation.widgetStringValue == "Second label"
            }
            print("[INFO] \(annotation.fieldName!): \(annotation.widgetStringValue ?? "<no string>")")
        default: break
        }
    }
    let image = page.thumbnail(of: NSSize(width: 800, height: 1000), for: .mediaBox)
    let bitmap = NSBitmapImageRep(data: image.tiffRepresentation!)!
    try bitmap.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("page-\(index).png"))
}
valid = valid && buttons == 2 && selectedButtons == 1 && choices == 2
print("[\(valid ? "PASS" : "FAIL")] PDFKit: \(buttons) radio buttons, \(selectedButtons) selected, \(choices) choice fields; dropdown label checked; both pages rendered")
exit(valid ? 0 : 1)
