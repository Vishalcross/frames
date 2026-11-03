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

        Settings {
            SettingsView()
                .environment(model)
                .themed(model.theme)
        }
    }
}

struct LibraryCommands: Commands {
    let model: AppModel

    var body: some Commands {
        CommandGroup(after: .newItem) {
            Button("Import…") { Task { await model.chooseImport() } }
                .keyboardShortcut("i", modifiers: [.command, .shift])
                .disabled(model.importRunning)
            Divider()
            Button("New Album") { model.newAlbum(with: []) }
                .keyboardShortcut("n")
            Button("New Album with Selection") { model.newAlbum(with: model.selectedIDs) }
                .keyboardShortcut("n", modifiers: [.command, .option])
                .disabled(model.selection.isEmpty)
            Divider()
            Button("Export Originals…") { Task { await model.exportSelection() } }
                .keyboardShortcut("e", modifiers: [.command, .shift])
                .disabled(model.selection.isEmpty)
            Divider()
            Button("Sync Now") { model.syncNow() }
                .keyboardShortcut("r", modifiers: [.command, .shift])
                .disabled(model.connectionProblem != nil || model.workOffline)
            Toggle("Work Offline", isOn: Binding(get: { model.workOffline }, set: { model.workOffline = $0 }))
        }
        CommandGroup(after: .pasteboard) {
            Divider()
            if model.sidebar == .trash {
                Button("Recover") { Task { await model.restoreSelection() } }
                    .disabled(model.selection.isEmpty)
                Button("Delete Now…") { model.confirmDeleteNow() }
                    .keyboardShortcut(.delete)
                    .disabled(model.selection.isEmpty)
            } else {
                Button("Move to Recently Deleted") { Task { await model.trashSelection() } }
                    .keyboardShortcut(.delete)
                    .disabled(model.selection.isEmpty)
            }
        }
        CommandMenu("Image") {
            Button("Adjust Date and Time…") { Task { await model.adjustDate() } }
                .disabled(model.selection.count != 1 && model.viewerIndex == nil)
            Button(model.showInfo ? "Hide Info" : "Show Info") { model.showInfo.toggle() }
                .keyboardShortcut("i")
        }
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
        }
    }
}
