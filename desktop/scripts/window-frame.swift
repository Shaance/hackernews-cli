// Print the on-screen frame of the named process's window as "X Y W H" in points.
//
// `screencapture -l <CGWindowID>` returns an empty buffer for a Metal-backed window in this
// environment, so verification captures the whole screen and crops to this frame instead.
// CGWindowListCopyWindowInfo is not accessibility-gated, so no assistive access is needed.
//
// The window is picked by layer: `--on-top` puts it at the floating level (3), and a normal
// window is at 0. Layer 3 wins when both exist, because that is the one on screen.
import CoreGraphics
import Foundation

let owner = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "hn-desktop"
guard let windows = CGWindowListCopyWindowInfo(.optionAll, kCGNullWindowID) as? [[String: Any]] else {
    FileHandle.standardError.write(Data("window-frame: CGWindowListCopyWindowInfo failed\n".utf8))
    exit(1)
}

var best: (layer: Int, area: Double, bounds: CGRect)?
for window in windows {
    guard let name = window[kCGWindowOwnerName as String] as? String, name == owner else { continue }
    guard let layer = window[kCGWindowLayer as String] as? Int, layer == 0 || layer == 3 else { continue }
    guard let raw = window[kCGWindowBounds as String] as? [String: Any] else { continue }
    let bounds = CGRect(
        x: raw["X"] as? Double ?? 0,
        y: raw["Y"] as? Double ?? 0,
        width: raw["Width"] as? Double ?? 0,
        height: raw["Height"] as? Double ?? 0
    )
    // Skip the 1x1 helper windows AppKit keeps around.
    let area = bounds.width * bounds.height
    if area < 10_000 { continue }
    // Keep the highest layer, and the largest window within a layer: the floating window
    // `--on-top` makes is the one on screen, and the app's real window is the big one.
    if let current = best {
        if layer < current.layer || (layer == current.layer && area <= current.area) { continue }
    }
    best = (layer, area, bounds)
}

guard let best else {
    FileHandle.standardError.write(Data("window-frame: no window owned by \(owner)\n".utf8))
    exit(2)
}
let b = best.bounds
print("\(Int(b.origin.x.rounded())) \(Int(b.origin.y.rounded())) \(Int(b.width.rounded())) \(Int(b.height.rounded()))")
