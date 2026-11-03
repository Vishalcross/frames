import SwiftUI

/// Frames › Settings (⌘,): connection, storage, library maintenance and appearance.
struct SettingsView: View {
    /// The pane last shown, so Settings opens where you left it.
    @AppStorage("FramesSettingsTab") private var tab = "appearance"

    var body: some View {
        TabView(selection: $tab) {
            AppearancePane().tabItem { Label("Appearance", systemImage: "paintpalette") }.tag("appearance")
            ConnectionPane().tabItem { Label("MEGA S4", systemImage: "icloud") }.tag("connection")
            StoragePane().tabItem { Label("Storage", systemImage: "internaldrive") }.tag("storage")
            LibraryPane().tabItem { Label("Library", systemImage: "photo.stack") }.tag("library")
        }
        .frame(width: 560)
    }
}

/// Standard, Adaptive or Glass, and their options (spec section 9).
struct AppearancePane: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var settings = model.theme.settings
        Form {
            Section {
                Picker("Appearance", selection: $settings.appearance) {
                    ForEach(Appearance.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                Text(settings.appearance.summary).font(.callout).foregroundStyle(.secondary)
            }
            Section {
                Picker("Colours", selection: $settings.scheme) {
                    ForEach(SchemeChoice.allCases.filter { $0 != .followPhoto || settings.appearance == .adaptive }) {
                        Text($0.title).tag($0)
                    }
                }
                if settings.appearance != .adaptive {
                    LabeledContent("Accent") {
                        HStack(spacing: 8) {
                            ForEach(AccentChoice.allCases) { choice in
                                AccentSwatch(choice: choice, selected: settings.accent == choice) { settings.accent = choice }
                            }
                        }
                    }
                }
            }
            if settings.appearance == .adaptive {
                Section("Adaptive") {
                    Picker("Intensity", selection: $settings.intensity) {
                        ForEach(AdaptiveIntensity.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    Picker("Take colours from", selection: $settings.colourSource) {
                        ForEach(ColourSource.allCases) { Text($0.title).tag($0) }
                    }
                    Text("“Follow Photo” turns the app dark for a dark photo and light for a bright one. "
                         + "Browsing changes the colours but never switches between light and dark.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .onChange(of: settings.appearance) { model.theme.settingsChanged() }
        .onChange(of: settings.scheme) {
            if settings.scheme == .followPhoto && settings.appearance != .adaptive { settings.scheme = .system }
            model.theme.settingsChanged()
        }
        .onChange(of: settings.accent) { model.theme.settingsChanged() }
        .onChange(of: settings.intensity) { model.theme.settingsChanged() }
        .onChange(of: settings.colourSource) { model.theme.settingsChanged() }
    }
}

struct AccentSwatch: View {
    let choice: AccentChoice
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Circle()
                .fill(choice.color.map { AnyShapeStyle($0) } ?? AnyShapeStyle(AngularGradient(colors: [.red, .orange, .yellow, .green, .blue, .purple, .red], center: .center)))
                .frame(width: 18, height: 18)
                .overlay { if selected { Circle().stroke(.primary, lineWidth: 2).padding(-3) } }
        }
        .buttonStyle(.plain)
        .help(choice == .system ? "Your Mac's accent colour" : choice.rawValue.capitalized)
    }
}

/// The MEGA S4 bucket. The same settings.json the Tauri app uses; the secret
/// goes to the Keychain and never comes back out.
struct ConnectionPane: View {
    @Environment(AppModel.self) private var model
    @State private var settings: LibrarySettings?
    @State private var secret = ""
    @State private var status: (text: String, ok: Bool)?
    @State private var busy = false

    var body: some View {
        Form {
            if let binding = Binding($settings) {
                Section {
                    Toggle("Sync with MEGA S4", isOn: binding.useS4)
                    TextField("Endpoint", text: binding.endpoint)
                    TextField("Region", text: binding.region)
                    TextField("Bucket", text: binding.bucket)
                }
                Section("Keys") {
                    TextField("Access Key ID", text: binding.accessKeyId)
                    SecureField("Secret Access Key", text: $secret,
                                prompt: Text(binding.wrappedValue.hasSecret ? "Saved in your Keychain" : "Required"))
                }
                Section {
                    HStack {
                        if let status {
                            Label(status.text, systemImage: status.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill")
                                .foregroundStyle(status.ok ? .green : .orange)
                                .lineLimit(3)
                        } else if let problem = model.connectionProblem {
                            Text(problem).foregroundStyle(.secondary)
                        } else {
                            Label("Connected", systemImage: "checkmark.icloud").foregroundStyle(.secondary)
                        }
                        Spacer()
                        if busy { ProgressView().controlSize(.small) }
                        Button("Test") { run(save: false) }.disabled(busy)
                        Button("Save") { run(save: true) }
                            .keyboardShortcut(.defaultAction)
                            .disabled(busy)
                    }
                }
            } else {
                ProgressView()
            }
        }
        .formStyle(.grouped)
        .task { settings = try? await model.core?.run { try $0.settings() } }
    }

    private func run(save: Bool) {
        guard let core = model.core, let current = settings else { return }
        let typed = secret.isEmpty ? nil : secret
        busy = true
        status = nil
        Task {
            do {
                if save {
                    try await core.run { try $0.saveSettings(settings: current, secret: typed) }
                    secret = ""
                    model.connectionChanged()
                    settings = try? await core.run { try $0.settings() }
                    status = model.connectionProblem.map { ($0, false) } ?? ("Saved and connected.", true)
                } else {
                    try await core.run { try $0.testConnection(settings: current, secret: typed) }
                    status = ("The bucket answered. These settings work.", true)
                }
            } catch let error as FramesError {
                status = (error.message, false)
            } catch {
                status = (error.localizedDescription, false)
            }
            busy = false
        }
    }
}

/// The space limit and the local cache.
struct StoragePane: View {
    @Environment(AppModel.self) private var model
    @State private var settings: LibrarySettings?
    @State private var cache: CacheInfo?
    @State private var message: String?

    var body: some View {
        Form {
            if let binding = Binding($settings) {
                Section("MEGA S4") {
                    Stepper(value: binding.spaceLimitGb, in: 1...100_000, step: 10) {
                        LabeledContent("Space limit", value: "\(binding.wrappedValue.spaceLimitGb) GB")
                    }
                    Text("Frames warns at 90% and stops importing when the bucket is full.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Section("Cache on this Mac") {
                    LabeledContent("Location") {
                        HStack {
                            Text(binding.wrappedValue.cacheLocation).lineLimit(1).truncationMode(.middle)
                            Button("Choose…") { chooseCache(binding) }
                        }
                    }
                    Stepper(value: binding.cacheLimitGb, in: 1...2_000) {
                        LabeledContent("Previews and originals", value: "up to \(binding.wrappedValue.cacheLimitGb) GB")
                    }
                    if let cache {
                        LabeledContent("Thumbnails", value: "\(cache.thumbnails.formatted()) · \(bytes(cache.thumbnailBytes))")
                        LabeledContent("Cached now", value: "\(bytes(cache.cachedBytes)) of \(bytes(cache.cachedLimitBytes))")
                    }
                    HStack {
                        Button("Clear Cache") { clear() }
                        Text("Thumbnails stay; previews and originals download again when needed.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                HStack {
                    if let message { Text(message).foregroundStyle(.secondary) }
                    Spacer()
                    Button("Save") { save() }.keyboardShortcut(.defaultAction)
                }
            } else {
                ProgressView()
            }
        }
        .formStyle(.grouped)
        .task { await load() }
    }

    private func load() async {
        settings = try? await model.core?.run { try $0.settings() }
        cache = try? await model.core?.run { try $0.cacheInfo() }
    }

    private func chooseCache(_ binding: Binding<LibrarySettings>) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        if panel.runModal() == .OK, let url = panel.url {
            binding.wrappedValue.cacheLocation = url.path
            message = "The new location is used the next time Frames opens."
        }
    }

    private func save() {
        guard let core = model.core, let current = settings else { return }
        Task {
            do {
                try await core.run { try $0.saveSettings(settings: current, secret: nil) }
                model.connectionChanged()
                message = "Saved."
            } catch let error as FramesError {
                message = error.message
            } catch {}
        }
    }

    private func clear() {
        guard let core = model.core else { return }
        Task {
            let freed = (try? await core.run { try $0.clearCache() }) ?? 0
            message = "Freed \(bytes(freed))."
            await load()
        }
    }

    private func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(count), countStyle: .file)
    }
}

/// Check the library against the bucket, and rebuild the local database.
struct LibraryPane: View {
    @Environment(AppModel.self) private var model
    @State private var settings: LibrarySettings?
    @State private var check: LibraryCheck?
    @State private var busy: String?
    @State private var message: String?
    @State private var confirmRebuild = false

    var body: some View {
        Form {
            if let settings {
                Section {
                    LabeledContent("This Mac", value: settings.deviceId)
                    LabeledContent("Library folder") {
                        Button("Show in Finder") { NSWorkspace.shared.open(URL(fileURLWithPath: settings.dataFolder)) }
                    }
                }
            }
            Section("Check Library") {
                Text("Compares your library with the bucket: originals that are missing, files nobody imported, and uploads left over from a crash.")
                    .font(.callout).foregroundStyle(.secondary)
                if let check {
                    LabeledContent("Missing originals", value: check.missingOriginals.formatted())
                    LabeledContent("Files not in the library", value: check.unknownFiles.count.formatted())
                    LabeledContent("Leftover uploads tidied", value: check.tidiedUploads.formatted())
                    if check.limitBytes > 0 {
                        LabeledContent("Space used", value: "\(bytes(check.usedBytes)) of \(bytes(check.limitBytes))")
                    }
                    if !check.unknownFiles.isEmpty {
                        Button("Add \(check.unknownFiles.count) Files to the Library") { addUnknown(check.unknownFiles) }
                            .disabled(busy != nil)
                    }
                }
                Button("Check Library") { runCheck() }.disabled(busy != nil)
            }
            Section("Rebuild") {
                Text("Replaces this Mac's library database with the copy in MEGA S4. Use it if the library looks wrong or won't open.")
                    .font(.callout).foregroundStyle(.secondary)
                Button("Rebuild Database…", role: .destructive) { confirmRebuild = true }.disabled(busy != nil)
            }
            if busy != nil || message != nil {
                HStack {
                    if let busy { ProgressView().controlSize(.small); Text(busy) }
                    if let message { Text(message).foregroundStyle(.secondary) }
                }
            }
        }
        .formStyle(.grouped)
        .task { settings = try? await model.core?.run { try $0.settings() } }
        .confirmationDialog("Rebuild the library database?", isPresented: $confirmRebuild) {
            Button("Rebuild", role: .destructive) { rebuild() }
        } message: {
            Text("Edits not yet uploaded are lost. Photos in MEGA S4 are not touched.")
        }
    }

    private func perform(_ label: String, _ work: @escaping @Sendable (Library) throws -> String) {
        guard let core = model.core else { return }
        busy = label
        message = nil
        Task {
            do {
                message = try await core.run(work)
            } catch let error as FramesError {
                message = error.message
            } catch {
                message = error.localizedDescription
            }
            busy = nil
            await model.reload()
        }
    }

    private func runCheck() {
        guard let core = model.core else { return }
        busy = "Checking…"
        message = nil
        Task {
            do {
                check = try await core.run { try $0.checkLibrary() }
            } catch let error as FramesError {
                message = error.message
            } catch {}
            busy = nil
        }
    }

    private func addUnknown(_ keys: [String]) {
        perform("Adding…") { "Added \(try $0.addUnknownFiles(keys: keys)) files." }
        check = nil
    }

    private func rebuild() {
        perform("Rebuilding…") { library in
            try library.rebuildDatabase()
            return "Rebuilt from MEGA S4."
        }
    }

    private func bytes(_ count: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(count), countStyle: .file)
    }
}
