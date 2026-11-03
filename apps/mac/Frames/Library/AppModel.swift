import AppKit
import SwiftUI

enum SidebarItem: Hashable {
    case library
    case videos
    case imports
    case trash
    case album(String)
}

enum LibraryGrouping: String, CaseIterable, Identifiable {
    case years = "Years"
    case months = "Months"
    case all = "All Photos"

    var id: Self { self }
}

/// One structured part of a search, shown as a token in the search field
/// (spec section 9). Tokens combine; at most one of each kind applies.
enum SearchToken: Hashable, Identifiable {
    case album(id: String, name: String)
    case month(year: Int, month: Int)
    case camera(String)
    case type(String)
    case filename(String)

    var id: Self { self }

    var label: String {
        switch self {
        case .album(_, let name): name
        case .month(let year, let month): "\(Calendar.current.standaloneMonthSymbols[month - 1]) \(year)"
        case .camera(let camera): camera
        case .type(let type): type.capitalized
        case .filename(let name): name
        }
    }

    var symbol: String {
        switch self {
        case .album: "rectangle.stack"
        case .month: "calendar"
        case .camera: "camera"
        case .type: "doc"
        case .filename: "doc.text"
        }
    }
}

/// A form shown over the window.
enum Sheet: Identifiable {
    case newAlbum(assetIDs: [String])
    case renameAlbum(AlbumSummary)
    case adjustDate(AssetDetail)

    var id: String {
        switch self {
        case .newAlbum: "new-album"
        case .renameAlbum(let album): "rename-\(album.id)"
        case .adjustDate(let detail): "date-\(detail.id)"
        }
    }
}

/// A question to confirm before something that can't be undone.
enum Confirmation: Identifiable {
    case deleteAlbum(AlbumSummary)
    case deleteNow(assetIDs: [String])
    case emptyTrash(TrashSummary)

    var id: String {
        switch self {
        case .deleteAlbum(let album): "delete-\(album.id)"
        case .deleteNow: "delete-now"
        case .emptyTrash: "empty-trash"
        }
    }
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
        didSet { if sidebar != oldValue { clearSelection(); Task { await loadTimeline() } } }
    }
    var grouping = LibraryGrouping.all
    var albums: [AlbumSummary] = []
    var timeline = Timeline.empty
    var columns = 7
    /// Selected tiles, by index into `timeline`.
    var selection = IndexSet()
    /// The tile Shift-click and the arrow keys extend from, and the one
    /// Return opens.
    var focus: Int?
    var viewerIndex: Int?
    var showInfo = false
    var scrollTarget: ScrollTarget?
    var searchText = "" {
        didSet { if searchText != oldValue { searchChanged() } }
    }
    var searchTokens: [SearchToken] = [] {
        didSet { if searchTokens != oldValue { searchChanged() } }
    }
    var suggestedTokens: [SearchToken] = []
    var sheet: Sheet?
    var confirmation: Confirmation?
    /// Changed to ask the window to open Settings (SwiftUI opens it, not the model).
    var settingsRequest = 0
    /// A message for the user, such as an export result or an error.
    var notice: String?
    /// Whether the last sync reached S4; nil until the first one finishes.
    var online: Bool?
    /// Why the library isn't syncing at all, if it isn't.
    var connectionProblem: String?
    private(set) var core: Core?
    private(set) var pipeline: ImagePipeline?
    private(set) var viewerCache: ViewerCache?
    let transitions = ViewerTransitions()
    let theme = ThemeEngine()
    var importStatus: ImportStatus?
    /// Bytes per second over the last half minute, for "time left".
    private(set) var importRate: Double?
    @ObservationIgnored private var importSamples: [(time: Date, bytes: UInt64)] = []
    @ObservationIgnored private var importRefresh: Task<Void, Never>?
    private var searchTask: Task<Void, Never>?

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
            let testStore = UserDefaults.standard.bool(forKey: "FramesTestStore")
            let core = try await Core.open(dataDirectory: Self.dataDirectory, cacheDirectory: Self.cacheDirectory,
                                           testStore: testStore)
            self.core = core
            pipeline = ImagePipeline(core: core)
            viewerCache = ViewerCache()
            connectionProblem = core.library.connectionProblem()
            await reload()
            await refreshImportStatus()
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

    var isSearching: Bool {
        !searchText.trimmingCharacters(in: .whitespaces).isEmpty || !searchTokens.isEmpty
    }

    private var filter: TimelineFilter {
        var filter = TimelineFilter(albumId: nil, kind: nil, trash: false, text: nil, year: nil, month: nil, camera: nil, fileExt: nil)
        if isSearching {
            filter.text = searchText
            for token in searchTokens {
                switch token {
                case .album(let id, _): filter.albumId = id
                case .month(let year, let month):
                    filter.year = Int32(year)
                    filter.month = UInt32(month)
                case .camera(let camera): filter.camera = camera
                case .type("photo"): filter.kind = .photo
                case .type("video"): filter.kind = .video
                case .type("live"): filter.kind = .live
                case .type(let ext): filter.fileExt = ext
                case .filename(let name): filter.text = [searchText, name].joined(separator: " ")
                }
            }
            return filter
        }
        switch sidebar {
        case .library, .imports: break
        case .videos: filter.kind = .video
        case .trash: filter.trash = true
        case .album(let id): filter.albumId = id
        }
        return filter
    }

    func loadTimeline() async {
        guard let core else { return }
        let filter = self.filter
        let started = ContinuousClock.now
        guard let snapshot = try? await core.run({ try $0.timeline(filter: filter) }) else { return }
        let fetched = ContinuousClock.now
        // Read the selection now, not before the wait: it may have changed meanwhile.
        let selectedIDs = selection.count <= 5_000 ? selection.map { timeline.id(at: $0) } : []
        let focusID = focus.map { timeline.id(at: $0) }
        let viewerID = viewerIndex.map { timeline.id(at: $0) }
        timeline = Timeline(snapshot)
        let indexes = timeline.indexes(of: selectedIDs + [focusID, viewerID].compactMap { $0 })
        selection = IndexSet(selectedIDs.compactMap { indexes[$0] })
        focus = focusID.flatMap { indexes[$0] }
        viewerIndex = viewerID.flatMap { indexes[$0] }
        #if DEBUG
        if UserDefaults.standard.bool(forKey: "FramesDebugTimings") {
            print("timeline: \(timeline.count) items, core \(fetched - started), Swift \(ContinuousClock.now - fetched)")
        }
        #endif
    }

    private func listen(to events: AsyncStream<CoreEvent>) async {
        for await event in events {
            switch event {
            case .libraryChanged: await reload()
            case .thumbReady(let id): pipeline?.thumbnailArrived(id)
            case .syncState(let online): self.online = online
            case .importChanged: scheduleImportRefresh()
            }
        }
    }

    func syncNow() {
        core?.library.syncNow()
    }

    /// After Settings saved a new connection.
    func connectionChanged() {
        connectionProblem = core?.library.connectionProblem()
        online = nil
    }

    // MARK: Imports

    var importRunning: Bool { importStatus?.running == true }

    /// Ask which files and folders to import, into the album being shown if any.
    func chooseImport() async {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = true
        panel.prompt = "Import"
        panel.message = currentAlbum.map { "Choose photos and videos to import into “\($0.name)”." }
            ?? "Choose photos and videos, or folders of them, to import."
        guard await panel.begin() == .OK else { return }
        await importFiles(panel.urls, into: currentAlbum)
    }

    func importFiles(_ urls: [URL], into album: AlbumSummary?) async {
        guard let core, !urls.isEmpty else { return }
        let paths = urls.map(\.path)
        do {
            try await core.run { try $0.importPaths(paths: paths, albumId: album?.id) }
            importSamples = []
            sidebar = .imports
        } catch let error as FramesError {
            notice = error.message
        } catch {
            notice = error.localizedDescription
        }
    }

    func pauseImport() { core?.library.pauseImport() }
    func resumeImport() { core?.library.resumeImport() }

    func cancelImport() async {
        guard let core else { return }
        try? await core.run { try $0.cancelImport() }
    }

    /// Progress arrives once per file; refresh at most twice a second.
    private func scheduleImportRefresh() {
        guard importRefresh == nil else { return }
        importRefresh = Task {
            try? await Task.sleep(for: .milliseconds(500))
            await refreshImportStatus()
            importRefresh = nil
        }
    }

    func refreshImportStatus() async {
        guard let core, let status = try? await core.run({ try $0.importStatus() }) else { return }
        let wasRunning = importStatus?.running == true
        importStatus = status
        if status.running {
            let now = Date()
            importSamples.append((now, status.completedBytes))
            importSamples.removeAll { now.timeIntervalSince($0.time) > 30 }
            if let first = importSamples.first, let last = importSamples.last, last.time.timeIntervalSince(first.time) > 3 {
                importRate = Double(last.bytes &- first.bytes) / last.time.timeIntervalSince(first.time)
            }
        } else {
            importSamples = []
            importRate = nil
        }
        // New photos appear while an import runs, and once it ends.
        if wasRunning || status.running { await reload() }
    }

    // MARK: Search

    private func searchChanged() {
        searchTask?.cancel()
        searchTask = Task {
            try? await Task.sleep(for: .milliseconds(200))
            guard !Task.isCancelled else { return }
            await loadSuggestions()
            clearSelection()
            await loadTimeline()
        }
    }

    private func loadSuggestions() async {
        let text = searchText.trimmingCharacters(in: .whitespaces)
        guard let core, !text.isEmpty else {
            suggestedTokens = []
            return
        }
        guard let found = try? await core.run({ try $0.searchSuggestions(text: text) }) else { return }
        var tokens: [SearchToken] = found.albums.map { .album(id: $0.id, name: $0.name) }
        tokens += found.dates.compactMap { date in
            let parts = date.split(separator: "-").compactMap { Int($0) }
            return parts.count == 2 ? .month(year: parts[0], month: parts[1]) : nil
        }
        tokens += found.cameras.map { .camera($0) }
        tokens += found.fileTypes.map { .type($0) }
        tokens += found.filenames.map { .filename($0) }
        suggestedTokens = tokens.filter { !searchTokens.contains($0) }
    }

    // MARK: Selection

    var selectedIDs: [String] { selection.map { timeline.id(at: $0) } }

    func clearSelection() {
        selection = []
        focus = nil
    }

    // MARK: Viewer

    func openViewer(at index: Int) {
        guard index >= 0, index < timeline.count else { return }
        selection = [index]
        focus = index
        transitions.prepareOpening(index: index)
        viewerIndex = index
        theme.viewerShows(timeline.id(at: index), core: core)
    }

    func closeViewer() {
        guard let index = viewerIndex else { return }
        selection = [index]
        focus = index
        transitions.close(index: index) { [weak self] in self?.viewerIndex = nil }
        theme.viewerShows(nil, core: core)
    }

    func step(_ delta: Int) {
        guard let viewerIndex else { return }
        let next = viewerIndex + delta
        guard next >= 0, next < timeline.count else { return }
        self.viewerIndex = next
        selection = [next]
        focus = next
        theme.viewerShows(timeline.id(at: next), core: core)
    }

    /// Browsing rested on a photo: Adaptive may take its colours.
    func browsing(_ index: Int) {
        guard index < timeline.count else { return }
        theme.browsing(timeline.id(at: index), core: core)
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

    var currentAlbum: AlbumSummary? {
        guard case .album(let id) = sidebar, !isSearching else { return nil }
        return albums.first { $0.id == id }
    }

    var title: String {
        if isSearching { return "Search" }
        switch sidebar {
        case .library: return "Library"
        case .videos: return "Videos"
        case .imports: return "Imports"
        case .trash: return "Recently Deleted"
        case .album: return currentAlbum?.name ?? "Album"
        }
    }

    // MARK: Edits

    /// Run an edit in the core, report any error, then reload what changed.
    private func edit(_ work: @escaping @Sendable (Library) throws -> Void) async -> Bool {
        guard let core else { return false }
        do {
            try await core.run(work)
        } catch let error as FramesError {
            notice = error.message
            return false
        } catch {
            notice = error.localizedDescription
            return false
        }
        await reload()
        return true
    }

    func newAlbum(with ids: [String]) {
        sheet = .newAlbum(assetIDs: ids)
    }

    func createAlbum(named name: String, with ids: [String]) async {
        guard let core else { return }
        do {
            let id = try await core.run { try $0.createAlbum(name: name, assetIds: ids) }
            albums = (try? await core.run { try $0.albums() }) ?? albums
            sidebar = .album(id)
        } catch let error as FramesError {
            notice = error.message
        } catch {
            notice = error.localizedDescription
        }
    }

    func renameAlbum(_ album: AlbumSummary, to name: String) async {
        _ = await edit { try $0.renameAlbum(albumId: album.id, name: name) }
    }

    func deleteAlbum(_ album: AlbumSummary) async {
        if sidebar == .album(album.id) { sidebar = .library }
        _ = await edit { try $0.deleteAlbum(albumId: album.id) }
    }

    func add(_ ids: [String], to album: AlbumSummary) async {
        guard !ids.isEmpty else { return }
        _ = await edit { try $0.addToAlbum(albumId: album.id, assetIds: ids) }
    }

    func removeSelectionFromAlbum() async {
        guard let album = currentAlbum else { return }
        let ids = selectedIDs
        clearSelection()
        _ = await edit { try $0.removeFromAlbum(albumId: album.id, assetIds: ids) }
    }

    func moveSelection(to album: AlbumSummary) async {
        guard let from = currentAlbum, from.id != album.id else { return }
        let ids = selectedIDs
        clearSelection()
        _ = await edit { try $0.moveToAlbum(fromAlbumId: from.id, toAlbumId: album.id, assetIds: ids) }
    }

    func trashSelection() async {
        let ids = selectedIDs
        guard !ids.isEmpty, sidebar != .trash else { return }
        if viewerIndex != nil { viewerIndex = nil }
        clearSelection()
        _ = await edit { try $0.trash(assetIds: ids) }
    }

    func restoreSelection() async {
        let ids = selectedIDs
        guard !ids.isEmpty else { return }
        clearSelection()
        _ = await edit { try $0.restore(assetIds: ids) }
    }

    func confirmDeleteNow() {
        let ids = selectedIDs
        guard !ids.isEmpty else { return }
        confirmation = .deleteNow(assetIDs: ids)
    }

    func deleteNow(_ ids: [String]) async {
        clearSelection()
        _ = await edit { _ = try $0.deleteNow(assetIds: ids) }
    }

    func confirmEmptyTrash() async {
        guard let core, let summary = try? await core.run({ try $0.trashSummary() }), summary.count > 0 else { return }
        confirmation = .emptyTrash(summary)
    }

    func emptyTrash() async {
        clearSelection()
        _ = await edit { _ = try $0.emptyTrash() }
    }

    /// Open the Adjust Date sheet for the focused photo.
    func adjustDate() async {
        guard let core, let index = viewerIndex ?? focus ?? selection.first else { return }
        let id = timeline.id(at: index)
        if let detail = try? await core.run({ try $0.assetDetail(assetId: id) }) {
            sheet = .adjustDate(detail)
        }
    }

    /// Save a new date taken, given as the wall-clock time where it was taken.
    func setDate(_ detail: AssetDetail, localTime: Date) async {
        let offsetMs = Int64(detail.tzOffsetMinutes ?? 0) * 60_000
        let utcMs = Int64(localTime.timeIntervalSince1970 * 1000) - offsetMs
        _ = await edit { try $0.setDate(assetId: detail.id, takenUtcMs: utcMs) }
    }

    /// Ask for a folder, then copy the selected originals into it.
    func exportSelection() async {
        let ids = selectedIDs
        guard let core, !ids.isEmpty else { return }
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Export"
        panel.message = ids.count == 1 ? "Choose where to save the original." : "Choose where to save \(ids.count) originals."
        guard await panel.begin() == .OK, let folder = panel.url else { return }
        do {
            let report = try await core.run { try $0.exportOriginals(assetIds: ids, folder: folder.path) }
            var message = "Exported \(report.exported) \(report.exported == 1 ? "original" : "originals")."
            if report.missing > 0 { message += " \(report.missing) could not be found on this Mac or in MEGA S4." }
            notice = message
        } catch let error as FramesError {
            notice = error.message
        } catch {
            notice = error.localizedDescription
        }
    }
}
