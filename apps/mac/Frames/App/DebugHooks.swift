#if DEBUG
import AppKit
import ImageIO

/// Test hooks for Debug builds, all off unless passed as launch arguments:
/// `-FramesDebugSidebar videos|trash|<album name>`, `-FramesDebugGrouping years|months|all`,
/// `-FramesDebugYear <year>` (Months from that year, as clicking it in Years does),
/// `-FramesDebugSelect <first>-<last>`, `-FramesDebugSearch <text>`,
/// `-FramesDebugColumns N`, `-FramesDebugOpen <index>` (opens the viewer),
/// `-FramesDebugInfo YES`, `-FramesDebugScrollTo <index>`, `-FramesDebugTimings YES`,
/// `-FramesDebugKeys right,left,esc` (posts those keys once the viewer is open, printing where
/// it is after each; `-FramesDebugKeysFromGrid YES` gives the grid the keyboard first),
/// `-FramesDebugSwipe next,prev,short,hold` (two-finger swipes on the open photo: a full one each
/// way, a short slow one that should settle back, and one held mid-way for a snapshot),
/// `-FramesDebugImport <folder>`, `-FramesDebugAccessibility YES` (prints what VoiceOver hears in the grid), `-FramesDebugSettings YES` (opens Settings; pick
/// the pane with `-FramesSettingsTab appearance|connection|storage|library`),
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
        if defaults.integer(forKey: "FramesDebugYear") > 0 { model.showYear(defaults.integer(forKey: "FramesDebugYear")) }
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
        if let keys = defaults.string(forKey: "FramesDebugKeys") { await post(keys, model) }
        if let swipes = defaults.string(forKey: "FramesDebugSwipe") { await swipe(swipes, model) }
        if let folder = defaults.string(forKey: "FramesDebugImport") {
            await model.importFiles([URL(fileURLWithPath: folder)], into: nil)
        }
        if defaults.bool(forKey: "FramesDebugSettings") {
            model.settingsRequest += 1
        }
        if defaults.bool(forKey: "FramesDebugAccessibility"), let grid = model.transitions.grid {
            try? await Task.sleep(for: .milliseconds(500))
            let children = grid.accessibilityChildren() ?? []
            print("grid: \(grid.accessibilityRole()?.rawValue ?? "-") \"\(grid.accessibilityLabel() ?? "")\", \(children.count) children")
            for case let child as NSAccessibilityElement in children.prefix(4) {
                print("  \(child.accessibilityRole()?.rawValue ?? "-") \"\(child.accessibilityLabel() ?? "")\" selected=\(child.isAccessibilitySelected()) frame=\(child.accessibilityFrame())")
            }
            print("selected: \(grid.accessibilitySelectedChildren()?.count ?? 0)")
        }
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

    private static func swipe(_ swipes: String, _ model: AppModel) async {
        try? await Task.sleep(for: .milliseconds(1200))
        guard let view = model.transitions.zoomView else { return print("swipe: no photo open") }
        /// A trackpad scroll event; `fingers` is how far they moved, positive to the right.
        func event(fingers: Double, phase: Int64) -> NSEvent? {
            guard let cg = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 2, wheel1: 0, wheel2: 0, wheel3: 0) else { return nil }
            cg.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
            cg.setIntegerValueField(.scrollWheelEventScrollPhase, value: phase)
            let inverted = NSEvent(cgEvent: cg)?.isDirectionInvertedFromDevice ?? false
            let delta = inverted ? fingers : -fingers
            cg.setDoubleValueField(.scrollWheelEventPointDeltaAxis2, value: delta)
            cg.setDoubleValueField(.scrollWheelEventFixedPtDeltaAxis2, value: delta)
            return NSEvent(cgEvent: cg)
        }
        print("swipe: start at \(model.viewerIndex.map(String.init) ?? "-") of \(model.timeline.count)")
        for kind in swipes.split(separator: ",") {
            let (step, count, pause): (Double, Int, Int) = switch kind {
            case "next": (-30, 12, 16)
            case "prev": (30, 12, 16)
            case "short": (-10, 3, 60)
            default: (-25, 10, 16)  // hold
            }
            event(fingers: 0, phase: 1).map(view.scrollWheel)
            for _ in 0..<count {
                try? await Task.sleep(for: .milliseconds(pause))
                event(fingers: step, phase: 2).map(view.scrollWheel)
            }
            if kind == "hold" { return print("swipe: holding") }
            event(fingers: 0, phase: 4).map(view.scrollWheel)
            try? await Task.sleep(for: .milliseconds(600))
            print("swipe: \(kind) -> \(model.viewerIndex.map(String.init) ?? "-")")
        }
    }

    private static func post(_ keys: String, _ model: AppModel) async {
        try? await Task.sleep(for: .milliseconds(800))
        guard let window = NSApp.windows.first(where: { $0.identifier?.rawValue.hasPrefix("library") == true }) else {
            return print("keys: no library window")
        }
        if UserDefaults.standard.bool(forKey: "FramesDebugKeysFromGrid"), let grid = model.transitions.grid {
            window.makeFirstResponder(grid)
        }
        print("keys: \(model.title) (\(model.timeline.count) items), start at \(model.viewerIndex.map(String.init) ?? "grid"), keyboard in \(type(of: window.firstResponder!))")
        for key in keys.split(separator: ",") {
            let (code, character): (UInt16, Int) = switch key {
            case "left": (123, NSLeftArrowFunctionKey)
            case "right": (124, NSRightArrowFunctionKey)
            default: (53, 27)
            }
            let text = String(Character(UnicodeScalar(character)!))
            for type in [NSEvent.EventType.keyDown, .keyUp] {
                if let event = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: key == "esc" ? [] : [.function, .numericPad],
                                                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                                                context: nil, characters: text, charactersIgnoringModifiers: text,
                                                isARepeat: false, keyCode: code) {
                    NSApp.postEvent(event, atStart: false)
                }
            }
            try? await Task.sleep(for: .milliseconds(500))
            print("keys: \(key) -> \(model.viewerIndex.map(String.init) ?? "grid") (selection \(model.selection.map(String.init).joined(separator: ",")))")
        }
    }

    private static func snapshotIfAsked(_ defaults: UserDefaults) {
        guard let path = defaults.string(forKey: "FramesDebugSnapshot") else { return }
        let delay = defaults.double(forKey: "FramesDebugDelay")
        Task {
            try? await Task.sleep(for: .seconds(delay > 0 ? delay : 3))
            let visible = NSApp.windows.filter(\.isVisible)
            print("windows: \(visible.map { "\($0.title) \(Int($0.frame.width))x\(Int($0.frame.height))" })")
            // Settings, when asked for, is any window other than the library's.
            let window = defaults.bool(forKey: "FramesDebugSettings")
                ? visible.first { $0.identifier?.rawValue.hasPrefix("library") != true && $0.title != "Frames" }
                : visible.first
            if let window, let frame = window.contentView?.superview {
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
