import AVKit
import SwiftUI

/// A video in the viewer, played by AVKit from the original on this Mac or
/// straight from S4 (frames_swift_spec.md section 10). Formats macOS can't
/// play say so and offer the original instead.
struct VideoView: View {
    @Environment(AppModel.self) private var model
    let id: String
    @State private var player: AVPlayer?
    @State private var failed = false

    var body: some View {
        ZStack {
            Color.black
            if let player { PlayerView(player: player, controls: true) }
            if failed {
                ContentUnavailableView {
                    Label("Can't Play Here", systemImage: "play.slash")
                } description: {
                    Text("This video's format doesn't play on a Mac. Export the original to open it in another app.")
                } actions: {
                    Button("Export Original…") { Task { await model.exportSelection() } }
                }
                .foregroundStyle(.white)
            }
        }
        .task(id: id) { await load() }
        .onDisappear { player?.pause() }
    }

    private func load() async {
        player?.pause()
        player = nil
        failed = false
        guard let core = model.core, let url = await mediaURL(core: core, video: true, id: id) else {
            failed = true
            return
        }
        let item = AVPlayerItem(url: url)
        let next = AVPlayer(playerItem: item)
        player = next
        while item.status == .unknown, !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(100))
        }
        if item.status == .failed { failed = true } else { next.play() }
    }
}

/// The LIVE badge on a Live Photo: press and hold it to play the movie.
struct LiveBadge: View {
    @Environment(AppModel.self) private var model
    let id: String
    @State private var player: AVPlayer?
    @State private var playing = false

    var body: some View {
        ZStack(alignment: .topLeading) {
            if playing, let player {
                PlayerView(player: player, controls: false)
                    .allowsHitTesting(false)
            }
            Text("LIVE")
                .font(.caption.bold())
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(.ultraThinMaterial, in: Capsule())
                .padding(14)
                .onLongPressGesture(minimumDuration: 0.05, maximumDistance: 40) {} onPressingChanged: { pressing in
                    if pressing {
                        Task { await play() }
                    } else {
                        stop()
                    }
                }
                .help("Press and hold to play")
        }
        .onChange(of: id) { stop() }
    }

    private func play() async {
        if player == nil, let core = model.core, let url = await mediaURL(core: core, video: false, id: id) {
            player = AVPlayer(url: url)
        }
        await player?.seek(to: .zero)
        player?.play()
        playing = true
    }

    private func stop() {
        player?.pause()
        playing = false
    }
}

/// A file path from the core becomes a file URL; anything else is a presigned S4 link.
private func mediaURL(core: Core, video: Bool, id: String) async -> URL? {
    let source = try? await core.run { video ? $0.videoSource(assetId: id) : $0.motionSource(assetId: id) }
    guard let source = source ?? nil else { return nil }
    return source.hasPrefix("/") ? URL(fileURLWithPath: source) : URL(string: source)
}

/// AVKit's player view, with or without its controls.
struct PlayerView: NSViewRepresentable {
    let player: AVPlayer
    let controls: Bool

    func makeNSView(context: Context) -> AVPlayerView {
        let view = AVPlayerView()
        view.controlsStyle = controls ? .floating : .none
        view.videoGravity = .resizeAspect
        return view
    }

    func updateNSView(_ view: AVPlayerView, context: Context) {
        if view.player !== player { view.player = player }
    }
}
