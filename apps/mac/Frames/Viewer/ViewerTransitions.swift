import AppKit

/// The zoom between a grid tile and the viewer (frames_swift_spec.md 7.5).
/// The photo travels on a layer above the window while the viewer's black
/// background fades in or out under it.
@MainActor
@Observable
final class ViewerTransitions {
    enum Phase {
        case idle
        case opening
        case open
    }

    private(set) var phase = Phase.idle
    @ObservationIgnored weak var grid: GridView?
    @ObservationIgnored weak var zoomView: ZoomView?
    @ObservationIgnored private var start: (image: CGImage, frame: CGRect)?
    private static let duration = 0.32

    private var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    /// Note where the tile is before the viewer covers it.
    func prepareOpening(index: Int) {
        if !reduceMotion, let snapshot = grid?.tileSnapshot(index), let image = snapshot.image {
            start = (image, snapshot.frame)
            phase = .opening
        } else {
            start = nil
            phase = .open
        }
    }

    /// The viewer is laid out: zoom the tile's image up to where the photo will sit.
    func viewerAppeared(_ view: ZoomView) {
        zoomView = view
        guard phase == .opening, let start, let window = view.window else {
            phase = .open
            return
        }
        self.start = nil
        let area = view.convert(view.bounds, to: nil)
        let target = Self.fit(start.image, in: area)
        Overlay.run(in: window, image: start.image, area: area, from: start.frame, to: target, backdropFrom: 0, backdropTo: 1) {
            self.phase = .open
        }
    }

    /// Shrink the photo back into its tile, then call `closed`. The viewer is
    /// closed at once under the travelling photo.
    func close(index: Int, closed: @escaping () -> Void) {
        guard !reduceMotion, phase == .open, let view = zoomView, let window = view.window,
              let image = view.image, view.magnification <= 1.01 else {
            phase = .idle
            return closed()
        }
        let area = view.convert(view.bounds, to: nil)
        let from = Self.fit(image, in: area)
        grid?.reveal(index)
        guard let tile = grid?.tileSnapshot(index)?.frame else {
            phase = .idle
            return closed()
        }
        phase = .idle
        Overlay.run(in: window, image: image, area: area, from: from, to: tile, backdropFrom: 1, backdropTo: 0, endsInTile: true, closed)
    }

    static func fit(_ image: CGImage, in area: CGRect) -> CGRect {
        let scale = min(area.width / CGFloat(image.width), area.height / CGFloat(image.height))
        let size = CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
        return CGRect(x: area.midX - size.width / 2, y: area.midY - size.height / 2, width: size.width, height: size.height)
    }

    /// A short-lived view above the window's content that animates the photo.
    private final class Overlay: NSView {
        static func run(in window: NSWindow, image: CGImage, area: CGRect, from: CGRect, to: CGRect,
                        backdropFrom: Float, backdropTo: Float, endsInTile: Bool = false,
                        _ completion: @escaping () -> Void) {
            guard let content = window.contentView else { return completion() }
            let overlay = Overlay(frame: content.bounds)
            overlay.wantsLayer = true
            content.addSubview(overlay, positioned: .above, relativeTo: nil)

            let backdrop = CALayer()
            backdrop.backgroundColor = NSColor.black.cgColor
            backdrop.frame = overlay.convert(area, from: nil)
            backdrop.opacity = backdropFrom
            // The clip is the tile's square at one end and the whole photo at the other.
            let clip = CALayer()
            clip.masksToBounds = true
            let photo = CALayer()
            photo.contents = image
            photo.contentsGravity = .resize
            clip.addSublayer(photo)
            overlay.layer?.addSublayer(backdrop)
            overlay.layer?.addSublayer(clip)

            let start = overlay.convert(from, from: nil)
            let end = overlay.convert(to, from: nil)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            clip.frame = start
            photo.frame = Self.photoFrame(image, clip: start, fill: !endsInTile)
            CATransaction.commit()

            CATransaction.begin()
            CATransaction.setAnimationDuration(ViewerTransitions.duration)
            CATransaction.setAnimationTimingFunction(CAMediaTimingFunction(controlPoints: 0.2, 0.8, 0.2, 1))
            CATransaction.setCompletionBlock {
                completion()
                // One more frame so the viewer or grid underneath has drawn.
                DispatchQueue.main.async { overlay.removeFromSuperview() }
            }
            clip.frame = end
            photo.frame = Self.photoFrame(image, clip: end, fill: endsInTile)
            backdrop.opacity = backdropTo
            CATransaction.commit()
        }

        /// The photo inside its clip: filling a square tile, or exactly fitting the viewer.
        private static func photoFrame(_ image: CGImage, clip: CGRect, fill: Bool) -> CGRect {
            guard fill else { return CGRect(origin: .zero, size: clip.size) }
            let scale = max(clip.width / CGFloat(image.width), clip.height / CGFloat(image.height))
            let size = CGSize(width: CGFloat(image.width) * scale, height: CGFloat(image.height) * scale)
            return CGRect(x: (clip.width - size.width) / 2, y: (clip.height - size.height) / 2, width: size.width, height: size.height)
        }

        override func hitTest(_ point: NSPoint) -> NSView? { nil }
    }
}
