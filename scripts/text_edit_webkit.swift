// Native macOS WebKit runner for the generated Phase 5 font-preview page.
// swift scripts/text_edit_webkit.swift scratch/text-edit/index.html
// A bounded poll and a required report prevent a navigation error or an empty
// page from becoming a pass. The HTML contains only original synthetic fonts.
import AppKit
import WebKit
import PDFKit

guard CommandLine.arguments.count == 2 else {
    print("[FAIL] expected generated index.html path")
    exit(1)
}
let path = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
guard FileManager.default.fileExists(atPath: path.path) else {
    print("[FAIL] generated HTML is absent")
    exit(1)
}
// Check the same original glyphs through the OS PDF reader. This prevents a
// correctly loaded browser font from hiding a bad PDF fixture. When the Rust
// round-trip outputs exist, check the edited glyph order independently too.
for name in ["truetype", "opentype-cff"] {
    let root = path.deletingLastPathComponent()
    let edited = root.appendingPathComponent("\(name)-replace/B-surgical-set-text.pdf")
    var inputs = [(root.appendingPathComponent("\(name).pdf"), "AB")]
    if FileManager.default.fileExists(atPath: edited.path) { inputs.append((edited, "BA")) }
    for (url, expected) in inputs {
        guard let document = PDFDocument(url: url), document.pageCount == 1,
              let page = document.page(at: 0),
              page.string?.trimmingCharacters(in: .whitespacesAndNewlines) == expected,
              let tiff = page.thumbnail(of: NSSize(width: 300, height: 160), for: .mediaBox).tiffRepresentation,
              let bitmap = NSBitmapImageRep(data: tiff), bitmap.pixelsWide == 300, bitmap.pixelsHigh == 160,
              let first = bitmap.colorAt(x: 40, y: 90)?.usingColorSpace(.deviceRGB),
              let second = bitmap.colorAt(x: 100, y: 90)?.usingColorSpace(.deviceRGB),
              (first.redComponent < 0.1) == (expected == "AB"),
              (second.redComponent < 0.1) == (expected == "BA") else {
            print("[FAIL] PDFKit glyphs or text: \(url.lastPathComponent) (\(name))")
            exit(1)
        }
        print("[PASS] PDFKit \(name): \(expected) text and glyph order")
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 60, y: 60, width: 650, height: 800),
                      styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
window.title = "tpdf text editing feasibility"
let webview = WKWebView(frame: window.contentView!.bounds)
webview.autoresizingMask = [.width, .height]
window.contentView!.addSubview(webview)
window.orderFrontRegardless()
webview.loadFileURL(path, allowingReadAccessTo: path.deletingLastPathComponent())
let deadline = Date().addingTimeInterval(30)
var inFlight = false
let timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
    if Date() > deadline {
        print("[FAIL] WebKit did not return a font report within 30 seconds")
        exit(1)
    }
    if inFlight { return }
    inFlight = true
    webview.evaluateJavaScript("window.fontProbeResult ? JSON.stringify(window.fontProbeResult) : null") { value, error in
        inFlight = false
        guard let text = value as? String, let data = text.data(using: .utf8),
              let report = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }
        print(text)
        let passed = report["passed"] as? Bool == true
            && (report["reports"] as? [[String: Any]])?.count == 13
            && (report["failures"] as? [String])?.isEmpty == true
        print(passed ? "[PASS] native WebKit font preview" : "[FAIL] native WebKit font preview")
        exit(passed ? 0 : 1)
    }
}
app.run()
