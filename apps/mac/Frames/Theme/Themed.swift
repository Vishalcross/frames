import SwiftUI

extension View {
    /// Apply the Appearance settings to the window: light or dark, the accent,
    /// and for Glass a see-through window (frames_swift_spec.md section 9).
    func themed(_ theme: ThemeEngine) -> some View {
        modifier(Themed(theme: theme))
    }
}

private struct Themed: ViewModifier {
    let theme: ThemeEngine
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        let colours = theme.colours
        let glass = theme.settings.appearance == .glass && !reduceTransparency
        content
            .preferredColorScheme(colours.scheme)
            .tint(colours.accent.map { Color(nsColor: $0) })
            .containerBackground(for: .window) {
                if glass {
                    ZStack {
                        Rectangle().fill(theme.settings.glassClarity == .clear ? AnyShapeStyle(.ultraThinMaterial) : AnyShapeStyle(.regularMaterial))
                        if let tint = colours.glassTint { Color(nsColor: tint).opacity(0.18) }
                    }
                } else {
                    Rectangle().fill(.windowBackground)
                }
            }
    }
}
