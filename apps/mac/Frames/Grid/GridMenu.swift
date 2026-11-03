import AppKit

/// The grid's right-click menu for the current selection.
@MainActor
struct GridMenu {
    let model: AppModel

    func build() -> NSMenu {
        let menu = NSMenu()
        let count = model.selection.count
        if model.sidebar == .trash && !model.isSearching {
            menu.addItem(item("Recover") { await model.restoreSelection() })
            menu.addItem(item("Delete Now…") { model.confirmDeleteNow() })
            return menu
        }
        if count == 1, let index = model.selection.first {
            menu.addItem(item("Open") { model.openViewer(at: index) })
        }
        menu.addItem(item("Get Info") { model.showInfo = true })
        if count == 1 {
            menu.addItem(item("Adjust Date and Time…") { await model.adjustDate() })
        }
        menu.addItem(.separator())
        menu.addItem(albumSubmenu("Add to Album", includeNew: true) { album in await model.add(model.selectedIDs, to: album) })
        if let current = model.currentAlbum {
            menu.addItem(albumSubmenu("Move to Album", includeNew: false, except: current.id) { album in await model.moveSelection(to: album) })
            menu.addItem(item("Remove from Album") { await model.removeSelectionFromAlbum() })
        }
        menu.addItem(item(count == 1 ? "Export Original…" : "Export \(count) Originals…") { await model.exportSelection() })
        menu.addItem(.separator())
        menu.addItem(item(count == 1 ? "Move to Recently Deleted" : "Move \(count) Items to Recently Deleted") { await model.trashSelection() })
        return menu
    }

    private func albumSubmenu(_ title: String, includeNew: Bool, except: String? = nil,
                              action: @escaping (AlbumSummary) async -> Void) -> NSMenuItem {
        let parent = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        let submenu = NSMenu()
        if includeNew {
            submenu.addItem(item("New Album…") { model.newAlbum(with: model.selectedIDs) })
            submenu.addItem(.separator())
        }
        for album in model.albums where album.id != except {
            submenu.addItem(item(album.name) { await action(album) })
        }
        parent.submenu = submenu
        parent.isEnabled = !submenu.items.isEmpty
        return parent
    }

    private func item(_ title: String, _ action: @escaping @MainActor () async -> Void) -> NSMenuItem {
        ActionMenuItem(title: title, action: action)
    }
}

/// A menu item that runs a closure.
@MainActor
final class ActionMenuItem: NSMenuItem {
    private let run: @MainActor () async -> Void

    init(title: String, action: @escaping @MainActor () async -> Void) {
        run = action
        super.init(title: title, action: #selector(runAction), keyEquivalent: "")
        target = self
    }

    required init(coder: NSCoder) { fatalError("not used") }

    @objc private func runAction() {
        let run = self.run
        Task { @MainActor in await run() }
    }
}
