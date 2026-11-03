import SwiftUI

/// The three looks from the Tauri app's Appearance settings.
enum Appearance: String, CaseIterable, Identifiable {
    /// macOS's own colours: the look of Photos.
    case standard
    /// The app takes its colours from the photo you are looking at.
    case adaptive
    /// See-through window and bars over the desktop.
    case glass

    var id: Self { self }

    var title: String {
        switch self {
        case .standard: "Standard"
        case .adaptive: "Adaptive"
        case .glass: "Glass"
        }
    }

    var summary: String {
        switch self {
        case .standard: "macOS colours, like Photos."
        case .adaptive: "Colours taken from the photo you're looking at."
        case .glass: "A see-through window over your desktop."
        }
    }
}

enum SchemeChoice: String, CaseIterable, Identifiable {
    case system, light, dark
    /// Adaptive only: dark for a dark photo, light for a bright one.
    case followPhoto

    var id: Self { self }

    var title: String {
        switch self {
        case .system: "Automatic"
        case .light: "Light"
        case .dark: "Dark"
        case .followPhoto: "Follow Photo"
        }
    }
}

enum AdaptiveIntensity: String, CaseIterable, Identifiable {
    case subtle, balanced, vivid

    var id: Self { self }
    var title: String { rawValue.capitalized }
    var factor: Double {
        switch self {
        case .subtle: 0.6
        case .balanced: 1
        case .vivid: 1.4
        }
    }
}

enum ColourSource: String, CaseIterable, Identifiable {
    /// Only an open photo colours the app.
    case viewer
    /// Also the photo under the pointer, or in the middle while scrolling.
    case viewerAndBrowsing

    var id: Self { self }

    var title: String {
        switch self {
        case .viewer: "Open photo only"
        case .viewerAndBrowsing: "Open photo and browsing"
        }
    }
}

enum AccentChoice: String, CaseIterable, Identifiable {
    case system, blue, purple, pink, red, orange, yellow, green, graphite

    var id: Self { self }

    var color: Color? {
        switch self {
        case .system: nil
        case .blue: .blue
        case .purple: .purple
        case .pink: .pink
        case .red: .red
        case .orange: .orange
        case .yellow: .yellow
        case .green: .green
        case .graphite: .gray
        }
    }
}

/// The Appearance settings, kept in UserDefaults on this Mac.
@MainActor
@Observable
final class ThemeSettings {
    var appearance: Appearance { didSet { save("appearance", appearance.rawValue) } }
    var scheme: SchemeChoice { didSet { save("scheme", scheme.rawValue) } }
    var accent: AccentChoice { didSet { save("accent", accent.rawValue) } }
    var intensity: AdaptiveIntensity { didSet { save("intensity", intensity.rawValue) } }
    var colourSource: ColourSource { didSet { save("colourSource", colourSource.rawValue) } }

    private static let prefix = "FramesTheme."

    init() {
        appearance = Self.load("appearance", Appearance.standard)
        scheme = Self.load("scheme", SchemeChoice.system)
        accent = Self.load("accent", AccentChoice.system)
        intensity = Self.load("intensity", AdaptiveIntensity.balanced)
        colourSource = Self.load("colourSource", ColourSource.viewerAndBrowsing)
    }

    private static func load<T: RawRepresentable<String>>(_ key: String, _ fallback: T) -> T {
        guard let raw = UserDefaults.standard.string(forKey: prefix + key), let value = T(rawValue: raw) else { return fallback }
        return value
    }

    private func save(_ key: String, _ value: String) {
        UserDefaults.standard.set(value, forKey: Self.prefix + key)
    }
}
