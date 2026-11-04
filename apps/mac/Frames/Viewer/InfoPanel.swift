import SwiftUI

/// Details of the open photo, or of the selected one in the grid.
struct InfoPanel: View {
    @Environment(AppModel.self) private var model
    @State private var detail: AssetDetail?

    var body: some View {
        let index = model.viewerIndex ?? model.focus ?? model.selection.first
        let id = index.flatMap { $0 < model.timeline.count ? model.timeline.id(at: $0) : nil }
        Group {
            if let detail, detail.id == id {
                Form {
                    Section {
                        Text(detail.filename).font(.headline).textSelection(.enabled)
                        LabeledContent("Taken", value: takenText(detail))
                        if detail.dateGuessed {
                            Text("This date was guessed from the file.").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                    Section {
                        if let camera = detail.camera { LabeledContent("Camera", value: camera) }
                        if let width = detail.width, let height = detail.height {
                            LabeledContent("Size", value: "\(width) × \(height)")
                        }
                        LabeledContent("File", value: ByteCountFormatter.string(fromByteCount: Int64(detail.size), countStyle: .file))
                        if let ms = detail.durationMs {
                            LabeledContent("Length", value: Duration.milliseconds(Int64(ms)).formatted(.time(pattern: .minuteSecond)))
                        }
                        LabeledContent("Kind", value: kindText(detail.kind))
                    }
                    if !detail.albums.isEmpty {
                        Section("Albums") {
                            ForEach(detail.albums, id: \.self) { Text($0) }
                        }
                    }
                }
                .formStyle(.grouped)
            } else if id == nil {
                ContentUnavailableView("No Selection", systemImage: "info.circle")
            } else {
                ProgressView()
            }
        }
        .task(id: id) {
            guard let id, let core = model.core else { return }
            detail = try? await core.run { try $0.assetDetail(assetId: id) }
        }
    }

    /// Wall-clock time where it was taken, with its UTC offset when known.
    private func takenText(_ detail: AssetDetail) -> String {
        let offset = Int(detail.tzOffsetMinutes ?? 0)
        let local = Date(timeIntervalSince1970: Double(detail.takenUtcMs) / 1000 + Double(offset * 60))
        let formatter = DateFormatter()
        formatter.timeZone = .gmt
        formatter.dateStyle = .long
        formatter.timeStyle = .short
        guard let minutes = detail.tzOffsetMinutes else { return formatter.string(from: local) }
        let sign = minutes < 0 ? "−" : "+"
        return formatter.string(from: local) + String(format: " (UTC%@%d:%02d)", sign, abs(Int(minutes)) / 60, abs(Int(minutes)) % 60)
    }

    private func kindText(_ kind: MediaKind) -> String {
        switch kind {
        case .photo: "Photo"
        case .video: "Video"
        case .live: "Live Photo"
        }
    }
}
