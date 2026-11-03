// Phase 0 grid trial (frames_swift_spec.md 7.3): the same 100k-item grid as
// a custom layer grid or an NSCollectionView, scrolled at fixed speeds.
// Usage: swift run -c release grid-spike --mode layers|collection [--items N] [--stay]
import AppKit

let arguments = CommandLine.arguments
func option(_ name: String) -> String? {
    guard let i = arguments.firstIndex(of: name), i + 1 < arguments.count else { return nil }
    return arguments[i + 1]
}
let mode = option("--mode") ?? "layers"
let itemCount = Int(option("--items") ?? "") ?? 100_000
let stayOpen = arguments.contains("--stay")
let launched = CACurrentMediaTime()

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    var collection: CollectionGrid?
    var benchmark: Benchmark?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let files = SyntheticLibrary.cachedThumbnails()
        guard !files.isEmpty else {
            print("No cached thumbnails found under ~/Library/Caches/app.frames.desktop.")
            exit(1)
        }
        let library = SyntheticLibrary(count: itemCount, files: files)
        let pipeline = ImagePipeline(library: library, costLimitMB: 300)

        let size = NSSize(width: 1280, height: 800)
        window = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                          styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Grid trial: \(mode)"
        let scrollView = NSScrollView(frame: NSRect(origin: .zero, size: size))
        scrollView.hasVerticalScroller = true
        scrollView.drawsBackground = true
        window.contentView = scrollView

        let width = size.width - 16
        if mode == "collection" {
            let grid = CollectionGrid(library: library, pipeline: pipeline, width: width, columns: 7)
            grid.attach(to: scrollView)
            collection = grid
        } else {
            scrollView.documentView = LayerGridView(library: library, pipeline: pipeline, width: width, columns: 7)
        }
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        scrollView.layoutSubtreeIfNeeded()
        let firstFrame = (CACurrentMediaTime() - launched) * 1000
        print(String(format: "mode=%@ items=%d sections=%d files=%d first-layout=%.0fms memory=%.0fMB",
                     mode, library.count, library.sections.count, files.count, firstFrame, memoryFootprintMB()))
        if stayOpen { return }

        let bench = Benchmark(scrollView: scrollView, pipeline: pipeline)
        benchmark = bench
        bench.run(speeds: [1, 2, 6, 20], secondsEach: 5) { results in
            for r in results {
                var line = String(format: "speed=%4.0f screens/s frames=%3d p50=%5.1fms p95=%5.1fms worst=%5.1fms hitches=%2d hitch=%5.1fms/s main-p95=%4.2fms main-worst=%5.2fms",
                                  r.speed, r.frames, r.p50, r.p95, r.worst, r.hitches, r.hitchMsPerSecond, r.mainP95, r.mainWorst)
                if let p50 = r.decodeP50, let p95 = r.decodeP95 {
                    line += String(format: " decode-p50=%.0fms decode-p95=%.0fms", p50, p95)
                }
                print(line)
            }
            print(String(format: "memory=%.0fMB", memoryFootprintMB()))
            NSApp.terminate(nil)
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
