import SwiftUI

@main
struct FramesApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        Window("Frames", id: "library") {
            ContentView()
                .environment(model)
                .frame(minWidth: 900, minHeight: 600)
                .task {
                    await model.open()
                    #if DEBUG
                    await DebugHooks.run(model)
                    #endif
                }
        }
        .defaultSize(width: 1280, height: 820)
        .commands { LibraryCommands(model: model) }
    }
}

struct LibraryCommands: Commands {
    let model: AppModel

    var body: some Commands {
        CommandGroup(after: .sidebar) {
            Button("Years") { model.grouping = .years }
                .keyboardShortcut("1")
            Button("Months") { model.grouping = .months }
                .keyboardShortcut("2")
            Button("All Photos") { model.grouping = .all }
                .keyboardShortcut("3")
            Divider()
            Button("Zoom In") { model.zoom(by: -1) }
                .keyboardShortcut("+")
            Button("Zoom Out") { model.zoom(by: 1) }
                .keyboardShortcut("-")
            Divider()
            Button(model.showInfo ? "Hide Info" : "Show Info") { model.showInfo.toggle() }
                .keyboardShortcut("i")
        }
        CommandGroup(after: .newItem) {
            Button("Sync Now") { model.syncNow() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
                .disabled(model.connectionProblem != nil)
        }
    }
}
