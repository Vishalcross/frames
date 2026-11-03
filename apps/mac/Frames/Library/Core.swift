import Foundation

/// What the core reports from its background threads.
enum CoreEvent: Sendable {
    case libraryChanged
    case thumbReady(String)
    case syncState(online: Bool)
}

/// The Rust core (frames_swift_spec.md section 5). Its calls block on SQLite,
/// the disk or the network, so they run on a background queue, never on the
/// main thread.
final class Core: Sendable {
    let library: Library
    let events: AsyncStream<CoreEvent>
    private static let queue = DispatchQueue(label: "frames.core", qos: .userInitiated, attributes: .concurrent)

    private init(library: Library, events: AsyncStream<CoreEvent>) {
        self.library = library
        self.events = events
    }

    /// Open the library, taking the library lock, and start its background sync.
    static func open(dataDirectory: URL, cacheDirectory: URL?) async throws -> Core {
        let (events, continuation) = AsyncStream.makeStream(of: CoreEvent.self)
        let observer = EventForwarder(continuation: continuation)
        let library = try await perform {
            try Library.open(dataDir: dataDirectory.path, cacheDir: cacheDirectory?.path, observer: observer)
        }
        return Core(library: library, events: events)
    }

    /// Run `work` against the library on the background queue.
    func run<T: Sendable>(_ work: @escaping @Sendable (Library) throws -> T) async throws -> T {
        let library = self.library
        return try await Self.perform { try work(library) }
    }

    private static func perform<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { continuation.resume(with: Result { try work() }) }
        }
    }
}

/// Hands the core's callbacks to an AsyncStream and returns at once, so a
/// Rust thread never waits on Swift.
private final class EventForwarder: LibraryObserver {
    let continuation: AsyncStream<CoreEvent>.Continuation

    init(continuation: AsyncStream<CoreEvent>.Continuation) {
        self.continuation = continuation
    }

    func libraryChanged() { continuation.yield(.libraryChanged) }
    func thumbReady(assetId: String) { continuation.yield(.thumbReady(assetId)) }
    func syncState(online: Bool) { continuation.yield(.syncState(online: online)) }
}

extension FramesError {
    /// What to tell the user.
    var message: String {
        switch self {
        case .AlreadyOpen:
            "Frames is already open in another app. Quit it, then try again."
        case .DamagedDatabase(let message):
            "The library database could not be opened: \(message)"
        case .Other(let message):
            message
        }
    }
}
