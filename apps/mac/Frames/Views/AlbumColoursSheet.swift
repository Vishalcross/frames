import SwiftUI

/// An album's colours in the Adaptive appearance: each photo's own, or one
/// palette for the whole album, generated from its photos or chosen by hand.
struct AlbumColoursSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let album: AlbumSummary
    @State private var onePalette: Bool
    @State private var chosen: Bool
    @State private var generated: ColourSet?
    @State private var background: Color
    @State private var sidebar: Color
    @State private var accent: Color
    @State private var regenerating = false

    init(album: AlbumSummary, colours: AlbumColours) {
        self.album = album
        _onePalette = State(initialValue: colours.onePalette)
        _chosen = State(initialValue: colours.chosen)
        _generated = State(initialValue: colours.generated)
        let start = colours.chosenColours ?? colours.generated
        func colour(_ hex: String?, _ fallback: Color) -> Color { hex.flatMap(NSColor.init(hex:)).map(Color.init(nsColor:)) ?? fallback }
        _background = State(initialValue: colour(start?.bg, Color(red: 0.11, green: 0.12, blue: 0.16)))
        _sidebar = State(initialValue: colour(start?.sidebar, Color(red: 0.09, green: 0.10, blue: 0.13)))
        _accent = State(initialValue: colour(start?.accent, .blue))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Colours for “\(album.name)”").font(.headline)
            Picker("Colours", selection: $onePalette) {
                Text("Each photo's own").tag(false)
                Text("One palette for the album").tag(true)
            }
            .pickerStyle(.radioGroup)
            if onePalette {
                Picker("Palette", selection: $chosen) {
                    Text("Made from the album's photos").tag(false)
                    Text("Chosen by me").tag(true)
                }
                .pickerStyle(.segmented)
                if chosen {
                    ColorPicker("Background", selection: $background, supportsOpacity: false)
                    ColorPicker("Sidebar", selection: $sidebar, supportsOpacity: false)
                    ColorPicker("Accent", selection: $accent, supportsOpacity: false)
                } else {
                    HStack {
                        if let generated {
                            Swatches(set: generated)
                        } else {
                            Text("Made when you save.").foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Make Again") { regenerate() }.disabled(regenerating)
                    }
                }
            }
            Text("Shown when Appearance is set to Adaptive in Settings.")
                .font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { save() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 380)
    }

    private func regenerate() {
        regenerating = true
        Task {
            if let colours = await model.regenerateColours(of: album) { generated = colours.generated }
            regenerating = false
        }
    }

    private func save() {
        let set = chosen ? ColourSet(bg: hex(background), sidebar: hex(sidebar), accent: hex(accent)) : nil
        Task { await model.saveColours(of: album, onePalette: onePalette, chosen: chosen, colours: set) }
        dismiss()
    }

    private func hex(_ colour: Color) -> String {
        let c = NSColor(colour).usingColorSpace(.sRGB) ?? .black
        return String(format: "#%02X%02X%02X", Int(round(c.redComponent * 255)), Int(round(c.greenComponent * 255)),
                      Int(round(c.blueComponent * 255)))
    }
}

private struct Swatches: View {
    let set: ColourSet

    var body: some View {
        HStack(spacing: 6) {
            ForEach([set.bg, set.sidebar, set.accent], id: \.self) { hex in
                RoundedRectangle(cornerRadius: 4)
                    .fill(NSColor(hex: hex).map(Color.init(nsColor:)) ?? .gray)
                    .frame(width: 28, height: 20)
                    .overlay(RoundedRectangle(cornerRadius: 4).stroke(.separator))
            }
        }
    }
}
