import AppKit

/// Years down the right edge of the grid (spec section 9). It appears while
/// the grid scrolls or the pointer is at the edge. Click or drag to jump; a
/// bubble shows the month under the pointer.
final class YearScrubber: NSView {
    /// Each year's first month, as a fraction of the way down the grid.
    var marks: [(year: Int, fraction: CGFloat)] = [] {
        didSet { needsDisplay = true }
    }
    /// Where the visible area is, as a fraction, for the position marker.
    var position: CGFloat = 0 {
        didSet { if abs(position - oldValue) > 0.001 { needsDisplay = true } }
    }
    var onScrub: (CGFloat) -> Void = { _ in }
    /// The month at a fraction, for the bubble.
    var titleAt: (CGFloat) -> String = { _ in "" }

    private let bubble = NSTextField(labelWithString: "")
    private var scrubbing = false
    private var hovering = false
    private var hideTimer: Timer?

    override init(frame: NSRect) {
        super.init(frame: frame)
        alphaValue = 0
        bubble.font = .systemFont(ofSize: 12, weight: .semibold)
        bubble.alignment = .center
        bubble.wantsLayer = true
        bubble.layer?.cornerRadius = 6
        bubble.isHidden = true
        addSubview(bubble)
        addTrackingArea(NSTrackingArea(rect: .zero, options: [.mouseEnteredAndExited, .activeInKeyWindow, .inVisibleRect],
                                       owner: self, userInfo: nil))
    }

    required init?(coder: NSCoder) { fatalError("not used") }

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard !marks.isEmpty else { return }
        let inset: CGFloat = 12
        let usable = bounds.height - inset * 2
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 11, weight: .medium),
            .foregroundColor: NSColor.labelColor,
        ]
        var lastY = -CGFloat.infinity
        for mark in marks {
            let y = inset + mark.fraction * usable
            guard y - lastY >= 22 else { continue }
            lastY = y
            let text = String(mark.year) as NSString
            let size = text.size(withAttributes: attributes)
            let pill = CGRect(x: bounds.width - size.width - 18, y: y - size.height / 2 - 2, width: size.width + 12, height: size.height + 4)
            NSColor.windowBackgroundColor.withAlphaComponent(0.85).setFill()
            NSBezierPath(roundedRect: pill, xRadius: pill.height / 2, yRadius: pill.height / 2).fill()
            text.draw(at: CGPoint(x: pill.minX + 6, y: pill.minY + 2), withAttributes: attributes)
        }
        if hovering || scrubbing {
            NSColor.controlAccentColor.setFill()
            NSBezierPath(roundedRect: CGRect(x: bounds.width - 4, y: inset + position * usable - 10, width: 3, height: 20),
                         xRadius: 1.5, yRadius: 1.5).fill()
        }
    }

    /// Show the years for a moment, as the grid scrolls.
    func flash() {
        show(true)
        hideTimer?.invalidate()
        hideTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.hovering, !self.scrubbing else { return }
                self.show(false)
            }
        }
    }

    private func show(_ visible: Bool) {
        guard (alphaValue > 0) != visible else { return }
        NSAnimationContext.runAnimationGroup { context in
            context.duration = visible ? 0.15 : 0.4
            animator().alphaValue = visible ? 1 : 0
        }
    }

    override func mouseEntered(with event: NSEvent) {
        hovering = true
        show(true)
        needsDisplay = true
    }

    override func mouseExited(with event: NSEvent) {
        hovering = false
        needsDisplay = true
        flash()
    }

    override func mouseDown(with event: NSEvent) {
        scrubbing = true
        scrub(to: event)
    }

    override func mouseDragged(with event: NSEvent) {
        scrub(to: event)
    }

    override func mouseUp(with event: NSEvent) {
        scrubbing = false
        bubble.isHidden = true
        needsDisplay = true
        flash()
    }

    private func scrub(to event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        let inset: CGFloat = 12
        let fraction = min(1, max(0, (point.y - inset) / (bounds.height - inset * 2)))
        onScrub(fraction)
        bubble.stringValue = titleAt(fraction)
        bubble.sizeToFit()
        let size = CGSize(width: bubble.frame.width + 16, height: 22)
        bubble.frame = CGRect(x: -size.width - 4, y: point.y - size.height / 2, width: size.width, height: size.height)
        bubble.layer?.backgroundColor = NSColor.windowBackgroundColor.withAlphaComponent(0.95).cgColor
        bubble.isHidden = false
    }
}
