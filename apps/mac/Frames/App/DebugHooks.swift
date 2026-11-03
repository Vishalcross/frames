#if DEBUG
import AppKit
import ImageIO

/// Test hooks for Debug builds, all off unless passed as launch arguments:
/// `-FramesDebugSidebar videos|trash|<album name>`, `-FramesDebugGrouping years|months|all`,
/// `-FramesDebugSelect <first>-<last>`, `-FramesDebugSearch <text>`,
/// `-FramesDebugColumns N`, `-FramesDebugOpen <index>` (opens the viewer),
/// `-FramesDebugInfo YES`, `-FramesDebugScrollTo <index>`, `-FramesDebugTimings YES`
/// (prints how long loading took), and `-FramesDebugSnapshot <png path>`, which writes
/// the window to a PNG after `-FramesDebugDelay` seconds (default 3) and quits.
@MainActor
enum DebugHooks {
    static func run(_ model: AppModel) async {
        let defaults = UserDefaults.standard
        guard case .ready = model.state else { return snapshotIfAsked(defaults) }
        switch defaults.string(forKey: "FramesDebugSidebar") {
        case "videos"?: model.sidebar = .videos
        case "trash"?: model.sidebar = .trash
        case let name?: if let album = model.albums.first(where: { $0.name == name }) { model.sidebar = .album(album.id) }
        case nil: break
        }
        // Selections below refer to the timeline for this sidebar item.
        await model.loadTimeline()
        switch defaults.string(forKey: "FramesDebugGrouping") {
        case "years"?: model.grouping = .years
        case "months"?: model.grouping = .months
        default: break
        }
        if defaults.integer(forKey: "FramesDebugColumns") > 0 { model.columns = defaults.integer(forKey: "FramesDebugColumns") }
        try? await Task.sleep(for: .milliseconds(300))
        if let search = defaults.string(forKey: "FramesDebugSearch") {
            model.searchText = search
            try? await Task.sleep(for: .milliseconds(600))
        }
        if let range = defaults.string(forKey: "FramesDebugSelect")?.split(separator: "-").compactMap({ Int($0) }), range.count == 2 {
            model.selection = IndexSet(integersIn: range[0]...range[1])
            model.focus = range[1]
        }
        if defaults.object(forKey: "FramesDebugScrollTo") != nil {
            model.scrollTarget = ScrollTarget(index: defaults.integer(forKey: "FramesDebugScrollTo"))
        }
        if defaults.object(forKey: "FramesDebugOpen") != nil { model.openViewer(at: defaults.integer(forKey: "FramesDebugOpen")) }
        if defaults.bool(forKey: "FramesDebugInfo") { model.showInfo = true }
        snapshotIfAsked(defaults)
    }

    /// With `-FramesDebugTimings YES`, print how long after the process
    /// started `event` happened, the first time it does.
    static func mark(_ event: String) {
        guard UserDefaults.standard.bool(forKey: "FramesDebugTimings"), marked.insert(event).inserted else { return }
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var name = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&name, 4, &info, &size, nil, 0) == 0 else { return }
        let start = info.kp_proc.p_starttime
        let started = Double(start.tv_sec) + Double(start.tv_usec) / 1_000_000
        print(String(format: "%@: %.0f ms after launch", event, (Date().timeIntervalSince1970 - started) * 1000))
    }

    private static var marked: Set<String> = []

    private static func snapshotIfAsked(_ defaults: UserDefaults) {
        guard let path = defaults.string(forKey: "FramesDebugSnapshot") else { return }
        let delay = defaults.double(forKey: "FramesDebugDelay")
        Task {
            try? await Task.sleep(for: .seconds(delay > 0 ? delay : 3))
            if let window = NSApp.windows.first(where: \.isVisible), let frame = window.contentView?.superview {
                write(frame, scale: window.backingScaleFactor, to: URL(fileURLWithPath: path))
            }
            NSApp.terminate(nil)
        }
    }

    private static func write(_ view: NSView, scale: CGFloat, to url: URL) {
        view.displayIfNeeded()
        guard let layer = view.layer,
              let context = CGContext(data: nil, width: Int(view.bounds.width * scale), height: Int(view.bounds.height * scale),
                                      bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return }
        context.scaleBy(x: scale, y: scale)
        layer.render(in: context)
        guard let image = context.makeImage(),
              let destination = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil) else { return }
        CGImageDestinationAddImage(destination, image, nil)
        CGImageDestinationFinalize(destination)
    }
}
#endif
