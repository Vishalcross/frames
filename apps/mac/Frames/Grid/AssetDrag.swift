import AppKit
import UniformTypeIdentifiers

extension UTType {
    /// An asset id on the pasteboard, for dropping photos on a sidebar album.
    /// Declared in Info.plist.
    static let framesAssetID = UTType(exportedAs: "app.frames.asset-id")
}

extension NSPasteboard.PasteboardType {
    static let framesAssetID = NSPasteboard.PasteboardType(UTType.framesAssetID.identifier)
}

/// One dragged photo: a file promise for Finder and other apps, written from
/// the original when it is dropped, plus the asset id for drops inside Frames.
final class AssetFilePromise: NSFilePromiseProvider {
    let assetID: String

    init(assetID: String, filename: String, delegate: NSFilePromiseProviderDelegate) {
        self.assetID = assetID
        let type = UTType(filenameExtension: (filename as NSString).pathExtension) ?? .image
        super.init()
        self.fileType = type.identifier
        self.delegate = delegate
        self.userInfo = filename
    }

    override func writableTypes(for pasteboard: NSPasteboard) -> [NSPasteboard.PasteboardType] {
        super.writableTypes(for: pasteboard) + [.framesAssetID]
    }

    override func writingOptions(forType type: NSPasteboard.PasteboardType, pasteboard: NSPasteboard) -> NSPasteboard.WritingOptions {
        type == .framesAssetID ? [] : super.writingOptions(forType: type, pasteboard: pasteboard)
    }

    override func pasteboardPropertyList(forType type: NSPasteboard.PasteboardType) -> Any? {
        type == .framesAssetID ? Data(assetID.utf8) : super.pasteboardPropertyList(forType: type)
    }
}

/// Writes promised originals where they were dropped, off the main thread.
final class AssetPromiseWriter: NSObject, NSFilePromiseProviderDelegate {
    private let core: Core
    private let queue: OperationQueue = {
        let queue = OperationQueue()
        queue.qualityOfService = .userInitiated
        queue.maxConcurrentOperationCount = 4
        return queue
    }()

    init(core: Core) {
        self.core = core
    }

    func filePromiseProvider(_ provider: NSFilePromiseProvider, fileNameForType fileType: String) -> String {
        provider.userInfo as? String ?? "Photo"
    }

    func filePromiseProvider(_ provider: NSFilePromiseProvider, writePromiseTo url: URL,
                             completionHandler: @escaping (Error?) -> Void) {
        guard let promise = provider as? AssetFilePromise else { return completionHandler(nil) }
        do {
            try core.library.exportOriginal(assetId: promise.assetID, path: url.path)
            completionHandler(nil)
        } catch {
            completionHandler(error)
        }
    }

    func operationQueue(for provider: NSFilePromiseProvider) -> OperationQueue { queue }
}
