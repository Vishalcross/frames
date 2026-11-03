import SwiftUI

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
        .toolbar { LibraryToolbar() }
    }
}

struct SidebarView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        List(selection: $model.sidebar) {
            Section("Photos") {
                Label("Library", systemImage: "photo.on.rectangle").tag(SidebarItem.library)
                Label("Videos", systemImage: "video").tag(SidebarItem.videos)
            }
            Section("Albums") {
                ForEach(model.albums, id: \.id) { album in
                    Label(album.name, systemImage: "rectangle.stack")
                        .badge(Int(album.assetCount))
                        .tag(SidebarItem.album(album.id))
                }
            }
        }
        .listStyle(.sidebar)
    }
}

/// The grid, or Years and Months cards in the library.
struct DetailView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        if model.sidebar == .library && model.grouping == .years {
            YearsView()
        } else if model.sidebar == .library && model.grouping == .months {
            MonthsView()
        } else if model.timeline.count == 0 {
            ContentUnavailableView("No Photos", systemImage: "photo.on.rectangle",
                                   description: Text(model.sidebar == .videos ? "There are no videos yet." : "Nothing here yet."))
        } else if let pipeline = model.pipeline {
            TimelineGrid(timeline: model.timeline, pipeline: pipeline, columns: $model.columns,
                         selection: $model.selection, scrollTarget: model.scrollTarget) { index in
                model.openViewer(at: index)
            }
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
        } else if model.sidebar == .library {
            ToolbarItem(placement: .principal) {
                Picker("View", selection: $model.grouping) {
                    ForEach(LibraryGrouping.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                .fixedSize()
            }
        }
        ToolbarItemGroup(placement: .primaryAction) {
            if model.viewerIndex == nil {
                Button("Zoom Out", systemImage: "minus.magnifyingglass") { model.zoom(by: 1) }
                    .disabled(model.columns >= 15)
                Button("Zoom In", systemImage: "plus.magnifyingglass") { model.zoom(by: -1) }
                    .disabled(model.columns <= 3)
            }
            Button("Info", systemImage: "info.circle") { model.showInfo.toggle() }
                .disabled(model.viewerIndex == nil && model.selection == nil)
            SyncButton()
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
