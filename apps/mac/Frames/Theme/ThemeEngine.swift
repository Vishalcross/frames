import SwiftUI

/// The colours the window uses right now (frames_swift_spec.md section 9).
struct ThemeColours: Equatable {
    var canvas: NSColor?
    var sidebar: NSColor?
    var accent: NSColor?
    var scheme: ColorScheme?
}

/// Works out the window's colours from the Appearance settings and, for
/// Adaptive, from the photo that decides them: the open photo, or while
/// browsing the photo under the pointer or in the middle of the scroll.
@MainActor
@Observable
final class ThemeEngine {
    let settings = ThemeSettings()
    private(set) var colours = ThemeColours()
    private var palette: PhotoPalette?
    /// Kept between photos, so mid-brightness photos don't flip it.
    private var photoScheme: ColorScheme = .dark
    @ObservationIgnored private var paletteRequest: String?
    @ObservationIgnored private var lastBrowseFocus = Date.distantPast

    init() {
        recompute()
    }

    /// The open photo changed (nil when the viewer closes).
    func viewerShows(_ id: String?, core: Core?) {
        guard settings.appearance == .adaptive else { return }
        if let id { focus(id, decidesScheme: true, core: core) } else if settings.colourSource == .viewer { setPalette(nil) }
    }

    /// The photo under the pointer, or in the middle of the scroll.
    func browsing(_ id: String, core: Core?) {
        guard settings.appearance == .adaptive, settings.colourSource == .viewerAndBrowsing,
              Date().timeIntervalSince(lastBrowseFocus) > 0.8 else { return }
        lastBrowseFocus = Date()
        // Browsing changes the colours, never light and dark (spec 9).
        focus(id, decidesScheme: false, core: core)
    }

    func settingsChanged() {
        if settings.appearance != .adaptive { palette = nil }
        recompute()
    }

    private func focus(_ id: String, decidesScheme: Bool, core: Core?) {
        guard let core, paletteRequest != id else { return }
        paletteRequest = id
        Task {
            let found = try? await core.run { $0.palette(assetId: id) }
            guard paletteRequest == id, let found else { return }
            if decidesScheme, let lum = found.lum {
                if lum < 0.45 { photoScheme = .dark } else if lum > 0.55 { photoScheme = .light }
            }
            setPalette(found)
        }
    }

    private func setPalette(_ new: PhotoPalette?) {
        palette = new
        withAnimation(.easeInOut(duration: NSWorkspace.shared.accessibilityDisplayShouldReduceMotion ? 0 : 0.4)) {
            recompute()
        }
    }

    private func recompute() {
        var next = ThemeColours()
        switch settings.scheme {
        case .system: next.scheme = nil
        case .light: next.scheme = .light
        case .dark: next.scheme = .dark
        case .followPhoto: next.scheme = settings.appearance == .adaptive ? photoScheme : nil
        }
        switch settings.appearance {
        case .standard, .glass:
            next.accent = settings.accent.color.map(NSColor.init)
        case .adaptive:
            let dark = (next.scheme ?? Self.systemScheme) == .dark
            if let palette, let roles = Self.immersive(palette, dark: dark, intensity: settings.intensity.factor) {
                next.canvas = roles.canvas
                next.sidebar = roles.sidebar
                next.accent = roles.accent
            } else {
                // Before any photo: a quiet slate in the same tones.
                let slate = Oklch(l: dark ? 0.22 : 0.965, c: 0.02, h: 255)
                next.canvas = slate.nsColor
                next.sidebar = Oklch(l: dark ? 0.185 : 0.925, c: 0.025, h: 255).nsColor
            }
        }
        if next != colours { colours = next }
    }

    private static var systemScheme: ColorScheme {
        NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? .dark : .light
    }

    /// The Tauri app's `immersiveRoles`: canvas in the photo's dominant hue,
    /// sidebar in its second hue a shade deeper, accent in its most vivid colour.
    static func immersive(_ p: PhotoPalette, dark: Bool, intensity k: Double)
        -> (canvas: NSColor, sidebar: NSColor, accent: NSColor)? {
        guard let bg = NSColor(hex: p.bg), let side = NSColor(hex: p.sidebar), let acc = NSColor(hex: p.accent) else { return nil }
        let vivid = acc.oklch
        func colourfulness(_ c: NSColor) -> Double { min(1, c.hslSaturation / 0.35) }
        let greyCanvas = colourfulness(bg) < 0.15
        let canvasHue = greyCanvas ? vivid.h : bg.oklch.h
        let canvasAmount = greyCanvas ? 0.3 : colourfulness(bg)
        let greySidebar = colourfulness(side) < 0.15
        let sidebarHue = greySidebar ? canvasHue : side.oklch.h
        let sidebarAmount = greySidebar ? canvasAmount : colourfulness(side)
        func surfaceChroma(_ amount: Double) -> Double { k * (dark ? 0.03 + 0.035 * amount : 0.012 + 0.022 * amount) }

        let canvas = Oklch(l: dark ? 0.22 : 0.965, c: surfaceChroma(canvasAmount), h: canvasHue).fitted
        let sidebar = Oklch(l: dark ? 0.185 : 0.925, c: surfaceChroma(sidebarAmount) * (dark ? 1 : 1.15), h: sidebarHue).fitted
        var accent = Oklch(l: dark ? 0.76 : 0.52, c: min(0.2, k * max(0.12, vivid.c)), h: vivid.h).fitted
        // At least 3:1 against the canvas, so the accent reads as a control.
        func contrast(_ a: Oklch, _ b: Oklch) -> Double {
            let (x, y) = (a.luminance, b.luminance)
            return (max(x, y) + 0.05) / (min(x, y) + 0.05)
        }
        var steps = 0
        while contrast(accent, canvas) < 3, steps < 40 {
            accent.l = min(0.98, max(0.02, accent.l + (dark ? 0.02 : -0.02)))
            steps += 1
        }
        return (canvas.nsColor, sidebar.nsColor, accent.nsColor)
    }
}
