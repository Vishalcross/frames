import SwiftUI

/// Name a new album, or rename one.
struct AlbumNameSheet: View {
    @Environment(\.dismiss) private var dismiss
    let title: String
    let action: String
    @State var name: String
    let save: (String) -> Void

    init(title: String, action: String, name: String, save: @escaping (String) -> Void) {
        self.title = title
        self.action = action
        _name = State(initialValue: name)
        self.save = save
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title).font(.headline)
            TextField("Album name", text: $name)
                .textFieldStyle(.roundedBorder)
                .frame(width: 280)
                .onSubmit(commit)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(action, action: commit)
                    .keyboardShortcut(.defaultAction)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(20)
    }

    private func commit() {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        save(trimmed)
        dismiss()
    }
}

/// Change when a photo was taken, as the clock read where it was taken.
/// Only the library changes; the file keeps its own date.
struct AdjustDateSheet: View {
    @Environment(\.dismiss) private var dismiss
    let detail: AssetDetail
    let save: (Date) -> Void
    @State private var date: Date

    init(detail: AssetDetail, save: @escaping (Date) -> Void) {
        self.detail = detail
        self.save = save
        let offset = Double(detail.tzOffsetMinutes ?? 0) * 60
        _date = State(initialValue: Date(timeIntervalSince1970: Double(detail.takenUtcMs) / 1000 + offset))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Adjust Date and Time").font(.headline)
            Text(detail.filename).foregroundStyle(.secondary)
            DatePicker("Taken", selection: $date, displayedComponents: [.date, .hourAndMinute])
                .datePickerStyle(.field)
                .environment(\.timeZone, .gmt)
            Text("The file itself isn't changed.").font(.caption).foregroundStyle(.secondary)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Adjust") {
                    save(date)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 340)
    }
}
