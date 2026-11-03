import SwiftUI

/// The colours the window uses right now (frames_swift_spec.md section 9).
struct ThemeColours: Equatable {
    var canvas: NSColor?
    var sidebar: NSColor?
    var accent: NSColor?
    var scheme: ColorScheme?
    /// Glass tinted from the photo.
    var glassTint: NSColor?
}

/// Works out the window's colours from the Appearance settings and, for
/// Adaptive (or Glass tinted from the photo), from what decides them: an
/// album's own palette when it has one, otherwise the open photo, or while
/// browsing the photo under the pointer or in the middle of the scroll.
@MainActor
@Observable
final class ThemeEngine {
    let settings = ThemeSettings()
    private(set) var colours = ThemeColours()
    /// The photo's palette, from the viewer or browsing.
    private var photoPalette: PhotoPalette?
    /// An album's own palette, which wins over photos. Chosen colours are
    /// shown exactly; generated ones are toned like a photo's.
    private var albumPalette: (palette: PhotoPalette, exact: Bool)?
    /// The album page's palette, kept while a photo from another album is open.
    @ObservationIgnored private var pagePalette: (palette: PhotoPalette, exact: Bool)?
    /// Kept between photos, so mid-brightness photos don't flip it.
    private var photoScheme: ColorScheme = .dark
    @ObservationIgnored private var request: String?
    @ObservationIgnored private var lastBrowseFocus = Date.distantPast
    @ObservationIgnored private var albumColours: [String: AlbumColours] = [:]

    init() {
        recompute()
    }

    /// The album page shown changed (nil: not an album).
    func albumPage(_ albumID: String?, core: Core?) {
        Task {
            pagePalette = await palette(ofAlbum: albumID, core: core)
            albumPalette = pagePalette
            setPalette(albumPalette == nil ? photoPalette : nil)
        }
    }

    /// The open photo changed (nil when the viewer closes). `albumID` is the
    /// album it was opened from, if any; otherwise its own album's setting applies.
    func viewerShows(_ id: String?, albumID: String?, core: Core?) {
        guard settings.takesPhotoColours else { return }
        guard let id else {
            albumPalette = pagePalette
            if settings.colourSource == .viewer && albumPalette == nil { photoPalette = nil }
            return animate()
        }
        request = id
        Task {
            var album = albumID
            if album == nil, let core { album = try? await core.run { $0.albumOf(assetId: id) } }
            let fromAlbum = await palette(ofAlbum: album, core: core)
            guard request == id else { return }
            albumPalette = fromAlbum
            if fromAlbum == nil { await focus(id, decidesScheme: true, core: core) } else { animate() }
        }
    }

    /// The photo under the pointer, or in the middle of the scroll.
    func browsing(_ id: String, core: Core?) {
        guard settings.takesPhotoColours, settings.colourSource == .viewerAndBrowsing, albumPalette == nil,
              Date().timeIntervalSince(lastBrowseFocus) > 0.8 else { return }
        lastBrowseFocus = Date()
        request = id
        // Browsing changes the colours, never light and dark (spec 9).
        Task { await focus(id, decidesScheme: false, core: core) }
    }

    func settingsChanged() {
        if !settings.takesPhotoColours { photoPalette = nil }
        recompute()
    }

    /// An album's colour setting changed: forget what was loaded for it.
    func albumColoursChanged(_ albumID: String, current: String?, core: Core?) {
        albumColours[albumID] = nil
        if current == albumID { albumPage(albumID, core: core) }
    }

    private func palette(ofAlbum albumID: String?, core: Core?) async -> (palette: PhotoPalette, exact: Bool)? {
        guard let albumID, let core else { return nil }
        if albumColours[albumID] == nil {
            albumColours[albumID] = try? await core.run { try $0.albumColours(albumId: albumID) }
        }
        guard let colours = albumColours[albumID], colours.onePalette else { return nil }
        let set = colours.chosen ? colours.chosenColours : colours.generated
        guard let set else { return nil }
        return (PhotoPalette(bg: set.bg, sidebar: set.sidebar, accent: set.accent, lum: nil), colours.chosen)
    }

    private func focus(_ id: String, decidesScheme: Bool, core: Core?) async {
        guard let core, let found = try? await core.run({ $0.palette(assetId: id) }), request == id else { return }
        if decidesScheme, let lum = found.lum {
            if lum < 0.45 { photoScheme = .dark } else if lum > 0.55 { photoScheme = .light }
        }
        setPalette(found)
    }

    private func setPalette(_ new: PhotoPalette?) {
        photoPalette = new
        animate()
    }

    private func animate() {
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
        let source = albumPalette ?? photoPalette.map { ($0, false) }
        switch settings.appearance {
        case .standard:
            next.accent = settings.accent.color.map(NSColor.init)
        case .glass:
            next.accent = settings.accent.color.map(NSColor.init)
            if settings.glassTint == .photo, let source, let accent = NSColor(hex: source.0.accent) {
                let hue = accent.oklch
                next.glassTint = Oklch(l: 0.62, c: min(0.14, max(0.06, hue.c)), h: hue.h).nsColor
            }
        case .adaptive:
            if let (palette, exact) = source, exact, let bg = NSColor(hex: palette.bg) {
                // Colours chosen for an album are shown as chosen, and their
                // background decides light or dark (as in the Tauri app).
                next.canvas = bg
                next.sidebar = NSColor(hex: palette.sidebar)
                next.accent = NSColor(hex: palette.accent)
                next.scheme = bg.oklch.luminance < 0.18 ? .dark : .light
            } else {
                let dark = (next.scheme ?? Self.systemScheme) == .dark
                if let (palette, _) = source, let roles = Self.immersive(palette, dark: dark, intensity: settings.intensity.factor) {
                    next.canvas = roles.canvas
                    next.sidebar = roles.sidebar
                    next.accent = roles.accent
                } else {
                    // Before any photo: a quiet slate in the same tones.
                    next.canvas = Oklch(l: dark ? 0.22 : 0.965, c: 0.02, h: 255).nsColor
                    next.sidebar = Oklch(l: dark ? 0.185 : 0.925, c: 0.025, h: 255).nsColor
                }
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
