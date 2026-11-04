import SwiftUI

/// Imports in progress and the last one's results (spec sections 7 and 8).
struct ImportsView: View {
    @Environment(AppModel.self) private var model
    @State private var confirmCancel = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                if let status = model.importStatus {
                    progress(status)
                    if let last = status.last, !status.running { results(last) }
                }
                HStack {
                    Button("Import Photos…") { Task { await model.chooseImport() } }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.importRunning || model.connectionProblem != nil)
                    Text("Or drag photos and folders onto the window.")
                        .foregroundStyle(.secondary)
                }
            }
            .padding(24)
            .frame(maxWidth: 720, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .confirmationDialog("Stop this import?", isPresented: $confirmCancel) {
            Button("Stop Import", role: .destructive) { Task { await model.cancelImport() } }
        } message: {
            Text("Photos already uploaded stay in your library. The rest won't be imported.")
        }
    }

    @ViewBuilder
    private func progress(_ s: ImportStatus) -> some View {
        let waiting = s.totalFiles > s.completedFiles + s.duplicates + s.failed
            ? s.totalFiles - s.completedFiles - s.duplicates - s.failed : 0
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Label(headline(s, waiting: waiting), systemImage: symbol(s, waiting: waiting))
                        .font(.headline)
                    Spacer()
                    if s.running {
                        Button(s.paused ? "Resume" : "Pause") { s.paused ? model.resumeImport() : model.pauseImport() }
                        Button("Stop…", role: .destructive) { confirmCancel = true }
                    }
                }
                if s.totalFiles > 0 {
                    ProgressView(value: Double(s.completedBytes), total: Double(max(s.totalBytes, 1)))
                    HStack {
                        Text("\(s.completedFiles.formatted()) of \(s.totalFiles.formatted()) files · "
                             + "\(bytes(s.completedBytes)) of \(bytes(s.totalBytes))")
                        Spacer()
                        if let left = timeLeft(s) { Text(left) }
                    }
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    if s.duplicates > 0 || s.failed > 0 {
                        Text([s.duplicates > 0 ? "\(s.duplicates.formatted()) duplicates skipped" : nil,
                              s.failed > 0 ? "\(s.failed.formatted()) failed" : nil].compactMap { $0 }.joined(separator: " · "))
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                }
                if !s.running, waiting > 0, let problem = model.connectionProblem {
                    Text("\(waiting.formatted()) files are waiting to upload. \(problem)")
                        .font(.callout)
                        .foregroundStyle(.orange)
                }
            }
            .padding(6)
        }
    }

    @ViewBuilder
    private func results(_ last: ImportSummary) -> some View {
        GroupBox("Last import") {
            VStack(alignment: .leading, spacing: 8) {
                Text("\(last.imported.formatted()) imported, \(last.duplicates.formatted()) duplicates skipped.")
                if last.stoppedOffline { Text("It paused because MEGA S4 couldn't be reached; it carries on when it can.").foregroundStyle(.orange) }
                if last.stoppedSpaceFull { Text("It stopped because the bucket is full.").foregroundStyle(.red) }
                if !last.failures.isEmpty {
                    DisclosureGroup("\(last.failures.count) failed") {
                        ForEach(last.failures, id: \.path) { failure in
                            VStack(alignment: .leading) {
                                Text((failure.path as NSString).lastPathComponent)
                                Text(failure.reason).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                }
                if !last.skipped.isEmpty {
                    DisclosureGroup("\(last.skipped.count) skipped (not photos or videos)") {
                        ForEach(last.skipped, id: \.self) { Text(($0 as NSString).lastPathComponent).font(.callout) }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(6)
        }
    }

    private func headline(_ s: ImportStatus, waiting: UInt64) -> String {
        if s.running { return s.paused ? "Import paused" : "Importing…" }
        if waiting > 0 { return "\(waiting.formatted()) files waiting to upload" }
        return s.totalFiles > 0 ? "All imports finished" : "No imports yet"
    }

    private func symbol(_ s: ImportStatus, waiting: UInt64) -> String {
        if s.running { return s.paused ? "pause.circle" : "arrow.up.circle" }
        return waiting > 0 ? "clock" : "checkmark.circle"
    }

    private func timeLeft(_ s: ImportStatus) -> String? {
        guard s.running, !s.paused, let rate = model.importRate, rate > 0 else { return nil }
        let seconds = Double(s.totalBytes &- s.completedBytes) / rate
        let formatter = DateComponentsFormatter()
        formatter.unitsStyle = .full
        formatter.maximumUnitCount = 2
        formatter.allowedUnits = seconds > 3600 ? [.hour, .minute] : [.minute, .second]
        return formatter.string(from: seconds).map { "About \($0) left" }
    }

    private func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(count), countStyle: .file)
    }
}
