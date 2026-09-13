// macOS producer fixture, using CoreText's public drawing API without rewriting
// any PDF operators. Run: swift testdata/make_textedit_quartz.swift <output.pdf> [--colour]
// Compare the output with text-edit-probe --inspect before extending its grammar.
import Foundation
import CoreGraphics
import CoreText

guard CommandLine.arguments.count == 2 ||
      (CommandLine.arguments.count == 3 && CommandLine.arguments[2] == "--colour") else {
    fputs("usage: make_textedit_quartz.swift <output.pdf> [--colour]\n", stderr)
    exit(1)
}
let output = URL(fileURLWithPath: CommandLine.arguments[1])
var box = CGRect(x: 0, y: 0, width: 300, height: 240)
guard let context = CGContext(output as CFURL, mediaBox: &box, nil) else {
    fputs("[FAIL] cannot create PDF\n", stderr)
    exit(1)
}
context.beginPDFPage(nil)
let font = CTFontCreateWithName("Helvetica" as CFString, 12, nil)
for (index, (text, y)) in [("SYNTHETIC FIRST", 180.0), ("SYNTHETIC SECOND", 140.0)].enumerated() {
    var attributes: [NSAttributedString.Key: Any] = [
        NSAttributedString.Key(kCTFontAttributeName as String): font
    ]
    if CommandLine.arguments.count == 3 {
        attributes[NSAttributedString.Key(kCTForegroundColorAttributeName as String)] =
            CGColor(red: index == 0 ? 0.8 : 0.1, green: 0.2,
                    blue: index == 0 ? 0.1 : 0.8, alpha: 1)
    }
    let string = NSAttributedString(string: text, attributes: attributes)
    context.textPosition = CGPoint(x: 40, y: y)
    CTLineDraw(CTLineCreateWithAttributedString(string), context)
}
context.endPDFPage()
context.closePDF()
print("[OK] wrote synthetic Quartz text fixture")
