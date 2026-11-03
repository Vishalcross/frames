import SwiftUI

enum SidebarItem: Hashable {
    case library
    case videos
    case album(String)
}

enum LibraryGrouping: String, CaseIterable, Identifiable {
    case years = "Years"
    case months = "Months"
    case all = "All Photos"

    var id: Self { self }
}

/// The open library and everything the window shows about it.
@MainActor
@Observable
final class AppModel {
    enum State {
        case opening
        case ready
        case failed(String)
    }

    var state = State.opening
    var sidebar = SidebarItem.library {
        didSet { if sidebar != oldValue { Task { await loadTimeline() } } }
    }
    var grouping = LibraryGrouping.all
    var albums: [AlbumSummary] = []
    var timeline = Timeline.empty
    var columns = 7
    var selection: Int?
    var viewerIndex: Int?
    var showInfo = false
    var scrollTarget: ScrollTarget?
    /// Whether the last sync reached S4; nil until the first one finishes.
    var online: Bool?
    /// Why the library isn't syncing at all, if it isn't.
    var connectionProblem: String?
    private(set) var core: Core?
    private(set) var pipeline: ImagePipeline?
    private(set) var viewerCache: ViewerCache?

    /// The library folder shared with the Tauri app, or `-FramesDataDir` for tests.
    static var dataDirectory: URL {
        if let path = UserDefaults.standard.string(forKey: "FramesDataDir") { return URL(fileURLWithPath: path) }
        return URL.applicationSupportDirectory.appending(path: "app.frames.desktop")
    }

    /// `-FramesCacheDir` for tests; otherwise the core uses the settings.
    static var cacheDirectory: URL? {
        UserDefaults.standard.string(forKey: "FramesCacheDir").map { URL(fileURLWithPath: $0) }
    }

    func open() async {
        guard core == nil else { return }
        do {
            let core = try await Core.open(dataDirectory: Self.dataDirectory, cacheDirectory: Self.cacheDirectory)
            self.core = core
            pipeline = ImagePipeline(core: core)
            viewerCache = ViewerCache()
            connectionProblem = core.library.connectionProblem()
            await reload()
            state = .ready
            Task { await listen(to: core.events) }
        } catch let error as FramesError {
            state = .failed(error.message)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func reload() async {
        guard let core else { return }
        albums = (try? await core.run { try $0.albums() }) ?? albums
        await loadTimeline()
    }

    func loadTimeline() async {
        guard let core else { return }
        let filter: TimelineFilter
        switch sidebar {
        case .library: filter = TimelineFilter(albumId: nil, videosOnly: false)
        case .videos: filter = TimelineFilter(albumId: nil, videosOnly: true)
        case .album(let id): filter = TimelineFilter(albumId: id, videosOnly: false)
        }
        let started = ContinuousClock.now
        guard let snapshot = try? await core.run({ try $0.timeline(filter: filter) }) else { return }
        let fetched = ContinuousClock.now
        let selectedID = selection.map { timeline.id(at: $0) }
        let viewerID = viewerIndex.map { timeline.id(at: $0) }
        timeline = Timeline(snapshot)
        #if DEBUG
        if UserDefaults.standard.bool(forKey: "FramesDebugTimings") {
            print("timeline: \(timeline.count) items, core \(fetched - started), Swift \(ContinuousClock.now - fetched)")
        }
        #endif
        selection = selectedID.flatMap { timeline.index(of: $0) }
        viewerIndex = viewerID.flatMap { timeline.index(of: $0) }
    }

    private func listen(to events: AsyncStream<CoreEvent>) async {
        for await event in events {
            switch event {
            case .libraryChanged: await reload()
            case .thumbReady(let id): pipeline?.thumbnailArrived(id)
            case .syncState(let online): self.online = online
            }
        }
    }

    func syncNow() {
        core?.library.syncNow()
    }

    // MARK: Viewer

    func openViewer(at index: Int) {
        guard index >= 0, index < timeline.count else { return }
        selection = index
        viewerIndex = index
    }

    func closeViewer() {
        if let viewerIndex { selection = viewerIndex }
        viewerIndex = nil
    }

    func step(_ delta: Int) {
        guard let viewerIndex else { return }
        let next = viewerIndex + delta
        guard next >= 0, next < timeline.count else { return }
        self.viewerIndex = next
        selection = next
    }

    // MARK: Navigation

    /// Show All Photos scrolled to a month (clicking a Months or Years card).
    func showMonth(year: Int, month: Int) {
        grouping = .all
        if let section = timeline.sections.first(where: { $0.year == year && $0.month == month }) {
            scrollTarget = ScrollTarget(index: section.start)
        }
    }

    func zoom(by step: Int) {
        columns = min(15, max(3, columns + step))
    }

    var title: String {
        switch sidebar {
        case .library: "Library"
        case .videos: "Videos"
        case .album(let id): albums.first { $0.id == id }?.name ?? "Album"
        }
    }
}
