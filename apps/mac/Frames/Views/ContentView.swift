import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        switch model.state {
        case .opening:
            ProgressView("Opening your library…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed(let message):
            ContentUnavailableView("Can't Open the Library", systemImage: "exclamationmark.triangle", description: Text(message))
        case .ready:
            LibraryWindow()
        }
    }
}

/// Sidebar, the library or an album, and the viewer over them.
struct LibraryWindow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 180, ideal: 220, max: 300)
        } detail: {
            ZStack {
                VStack(spacing: 0) {
                    StatusBanner()
                    DetailView()
                }
                .background(model.theme.colours.canvas.map { Color(nsColor: $0) } ?? .clear)
                .dropDestination(for: URL.self) { urls, _ in
                    Task { await model.importFiles(urls, into: model.currentAlbum) }
                    return !urls.isEmpty
                }
                if let index = model.viewerIndex {
                    ViewerView(index: index)
                }
            }
            .inspector(isPresented: $model.showInfo) {
                InfoPanel()
                    .inspectorColumnWidth(min: 240, ideal: 280, max: 360)
            }
        }
        .navigationTitle(model.viewerIndex == nil ? model.title : "")
        .navigationSubtitle(subtitle)
        .toolbar { LibraryToolbar() }
        .themed(model.theme)
        .searchable(text: $model.searchText, tokens: $model.searchTokens, suggestedTokens: $model.suggestedTokens,
                    placement: .toolbar, prompt: "Search") { token in
            Label(token.label, systemImage: token.symbol)
        }
        .sheet(item: $model.sheet) { sheet in
            switch sheet {
            case .newAlbum(let ids): AlbumNameSheet(title: "New Album", action: "Create", name: "") { name in
                    Task { await model.createAlbum(named: name, with: ids) }
                }
            case .renameAlbum(let album): AlbumNameSheet(title: "Rename Album", action: "Rename", name: album.name) { name in
                    Task { await model.renameAlbum(album, to: name) }
                }
            case .adjustDate(let detail): AdjustDateSheet(detail: detail) { date in
                    Task { await model.setDate(detail, localTime: date) }
                }
            }
        }
        .confirmations()
        .alert(model.notice ?? "", isPresented: Binding(get: { model.notice != nil }, set: { if !$0 { model.notice = nil } })) {
            Button("OK") { model.notice = nil }
        }
    }

    private var subtitle: String {
        guard model.viewerIndex == nil, model.sidebar != .imports || model.isSearching else { return "" }
        if !model.selection.isEmpty { return "\(model.selection.count) selected" }
        let count = model.timeline.count
        return count == 0 ? "" : "\(count.formatted()) \(count == 1 ? "item" : "items")"
    }
}

struct SidebarView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        @Bindable var model = model
        List(selection: $model.sidebar) {
            Section("Photos") {
                Label("Library", systemImage: "photo.on.rectangle").tag(SidebarItem.library)
                Label("Videos", systemImage: "video").tag(SidebarItem.videos)
                ImportsRow().tag(SidebarItem.imports)
                Label("Recently Deleted", systemImage: "trash").tag(SidebarItem.trash)
            }
            Section {
                ForEach(model.albums, id: \.id) { album in
                    AlbumRow(album: album)
                }
            } header: {
                HStack {
                    Text("Albums")
                    Spacer()
                    Button("New Album", systemImage: "plus") { model.newAlbum(with: []) }
                        .labelStyle(.iconOnly)
                        .buttonStyle(.borderless)
                        .help("New Album")
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(model.theme.colours.sidebar == nil ? .automatic : .hidden)
        .background(model.theme.colours.sidebar.map { Color(nsColor: $0) } ?? .clear)
        .safeAreaInset(edge: .bottom) {
            Button { openSettings() } label: {
                Label("Settings", systemImage: "gearshape")
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .buttonStyle(.borderless)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .help("Connection, storage, library and appearance (⌘,)")
        }
        .onChange(of: model.settingsRequest) { openSettings() }
    }
}

/// Imports, with how far the current one has got.
struct ImportsRow: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let status = model.importStatus
        Label {
            HStack {
                Text("Imports")
                Spacer()
                if let status, status.running, status.totalBytes > 0 {
                    ProgressView(value: Double(status.completedBytes), total: Double(status.totalBytes))
                        .progressViewStyle(.circular)
                        .controlSize(.mini)
                }
            }
        } icon: {
            Image(systemName: "square.and.arrow.down")
        }
    }
}

/// An album in the sidebar: a drop target for photos, with Rename and Delete.
struct AlbumRow: View {
    @Environment(AppModel.self) private var model
    let album: AlbumSummary
    @State private var targeted = false

    var body: some View {
        Label(album.name, systemImage: "rectangle.stack")
            .badge(Int(album.assetCount))
            .tag(SidebarItem.album(album.id))
            .listRowBackground(targeted ? RoundedRectangle(cornerRadius: 6).fill(Color.accentColor.opacity(0.25)) : nil)
            .contextMenu {
                Button("Rename…") { model.sheet = .renameAlbum(album) }
                Button("Delete Album…", role: .destructive) { model.confirmation = .deleteAlbum(album) }
            }
            .onDrop(of: [.framesAssetID, .fileURL], isTargeted: $targeted) { providers in
                Task {
                    // Photos from the grid join the album; files from Finder are imported into it.
                    let ids = await Self.assetIDs(from: providers)
                    if !ids.isEmpty { await model.add(ids, to: album) }
                    let files = await Self.fileURLs(from: providers)
                    if !files.isEmpty { await model.importFiles(files, into: album) }
                }
                return true
            }
    }

    private static func fileURLs(from providers: [NSItemProvider]) async -> [URL] {
        var urls: [URL] = []
        for provider in providers where provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
            let url: URL? = await withCheckedContinuation { continuation in
                _ = provider.loadObject(ofClass: URL.self) { url, _ in continuation.resume(returning: url) }
            }
            if let url { urls.append(url) }
        }
        return urls
    }

    private static func assetIDs(from providers: [NSItemProvider]) async -> [String] {
        var ids: [String] = []
        for provider in providers where provider.hasItemConformingToTypeIdentifier(UTType.framesAssetID.identifier) {
            let data: Data? = await withCheckedContinuation { continuation in
                provider.loadDataRepresentation(forTypeIdentifier: UTType.framesAssetID.identifier) { data, _ in
                    continuation.resume(returning: data)
                }
            }
            if let data, let id = String(data: data, encoding: .utf8) { ids.append(id) }
        }
        return ids
    }
}

/// The grid, or Years and Months cards in the library.
struct DetailView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if model.isSearching {
            grid(empty: ContentUnavailableView.search)
        } else if model.sidebar == .imports {
            ImportsView()
        } else if model.sidebar == .library && model.grouping == .years {
            YearsView()
        } else if model.sidebar == .library && model.grouping == .months {
            MonthsView()
        } else if model.sidebar == .trash {
            grid(empty: ContentUnavailableView("Recently Deleted Is Empty", systemImage: "trash",
                                               description: Text("Deleted photos stay here for 30 days.")))
        } else {
            grid(empty: ContentUnavailableView("No Photos", systemImage: "photo.on.rectangle",
                                               description: Text(model.sidebar == .videos ? "There are no videos yet." : "Nothing here yet.")))
        }
    }

    @ViewBuilder
    private func grid(empty: ContentUnavailableView<some View, some View, some View>) -> some View {
        if model.timeline.count == 0 {
            empty
        } else if let pipeline = model.pipeline {
            TimelineGrid(timeline: model.timeline, pipeline: pipeline) { GridMenu(model: model).build() }
        }
    }
}

struct LibraryToolbar: ToolbarContent {
    @Environment(AppModel.self) private var model

    var body: some ToolbarContent {
        @Bindable var model = model
        if model.viewerIndex != nil {
            ToolbarItem(placement: .navigation) {
                Button("Back", systemImage: "chevron.left") { model.closeViewer() }
            }
        } else if model.sidebar == .library && !model.isSearching {
            ToolbarItem(placement: .principal) {
                Picker("View", selection: $model.grouping) {
                    ForEach(LibraryGrouping.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            if model.sidebar == .trash && !model.isSearching {
                if model.selection.isEmpty {
                    Button("Delete All") { Task { await model.confirmEmptyTrash() } }
                        .disabled(model.timeline.count == 0)
                } else {
                    Button("Recover", systemImage: "arrow.uturn.backward") { Task { await model.restoreSelection() } }
                    Button("Delete", systemImage: "trash", role: .destructive) { model.confirmDeleteNow() }
                }
            } else if !model.selection.isEmpty || model.viewerIndex != nil {
                AddToAlbumMenu()
                Button("Export Originals", systemImage: "square.and.arrow.up") { Task { await model.exportSelection() } }
                Button("Move to Recently Deleted", systemImage: "trash") { Task { await model.trashSelection() } }
            }
            if model.viewerIndex == nil {
                Button("Zoom Out", systemImage: "minus.magnifyingglass") { model.zoom(by: 1) }
                    .disabled(model.columns >= 15)
                Button("Zoom In", systemImage: "plus.magnifyingglass") { model.zoom(by: -1) }
                    .disabled(model.columns <= 3)
            }
            Button("Info", systemImage: "info.circle") { model.showInfo.toggle() }
                .disabled(model.viewerIndex == nil && model.selection.isEmpty)
            SyncButton()
        }
    }
}

struct AddToAlbumMenu: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Menu("Add to Album", systemImage: "rectangle.stack.badge.plus") {
            Button("New Album…") { model.newAlbum(with: model.selectedIDs) }
            Divider()
            ForEach(model.albums, id: \.id) { album in
                Button(album.name) { Task { await model.add(model.selectedIDs, to: album) } }
            }
        }
    }
}

struct SyncButton: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Button { model.syncNow() } label: {
            Label("Sync", systemImage: symbol)
        }
        .help(help)
        .disabled(model.connectionProblem != nil)
    }

    private var symbol: String {
        if model.connectionProblem != nil { return "icloud.slash" }
        switch model.online {
        case true?: return "checkmark.icloud"
        case false?: return "exclamationmark.icloud"
        case nil: return "arrow.triangle.2.circlepath.icloud"
        }
    }

    private var help: String {
        if let problem = model.connectionProblem { return problem }
        switch model.online {
        case true?: return "Up to date with MEGA S4. Click to sync now."
        case false?: return "Offline. Browsing still works; Frames syncs when it can reach MEGA S4."
        case nil: return "Syncing with MEGA S4…"
        }
    }
}

/// Shown only when the library isn't syncing.
struct StatusBanner: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let text {
            Label(text, systemImage: "icloud.slash")
                .font(.callout)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .background(.bar)
        }
    }

    private var text: String? {
        if let problem = model.connectionProblem { return problem }
        if model.online == false { return "Offline. Your library is still here; it syncs when MEGA S4 is reachable." }
        return nil
    }
}

private struct Confirmations: ViewModifier {
    @Environment(AppModel.self) private var model

    func body(content: Content) -> some View {
        @Bindable var model = model
        content.alert(title, isPresented: Binding(get: { model.confirmation != nil }, set: { if !$0 { model.confirmation = nil } }),
                      presenting: model.confirmation) { confirmation in
            switch confirmation {
            case .deleteAlbum(let album):
                Button("Delete Album", role: .destructive) { Task { await model.deleteAlbum(album) } }
            case .deleteNow(let ids):
                Button("Delete", role: .destructive) { Task { await model.deleteNow(ids) } }
            case .emptyTrash:
                Button("Delete All", role: .destructive) { Task { await model.emptyTrash() } }
            }
            Button("Cancel", role: .cancel) {}
        } message: { confirmation in
            switch confirmation {
            case .deleteAlbum:
                Text("The photos stay in your library.")
            case .deleteNow(let ids):
                Text("\(ids.count) \(ids.count == 1 ? "item" : "items") will be deleted from MEGA S4 now. This can't be undone.")
            case .emptyTrash(let summary):
                Text("\(summary.count) \(summary.count == 1 ? "item" : "items") (\(ByteCountFormatter.string(fromByteCount: Int64(summary.bytes), countStyle: .file))) will be deleted from MEGA S4 now. This can't be undone.")
            }
        }
    }

    private var title: String {
        switch model.confirmation {
        case .deleteAlbum(let album)?: "Delete “\(album.name)”?"
        case .deleteNow?: "Delete Now?"
        case .emptyTrash?: "Delete Everything in Recently Deleted?"
        case nil: ""
        }
    }
}

extension View {
    func confirmations() -> some View { modifier(Confirmations()) }
}
