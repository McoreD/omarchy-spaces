import QtQuick
import "Spaces.mjs" as Spaces

// The frame around the workspaces, loaded with the shell: the tab strip along
// the top, and the spaces sidebar on the left, which the overlay entry
// (Toggle.qml) opens and closes so `super+p` goes through the shell like any
// other plugin. The strip comes first so the sidebar sits below it. Both are
// opaque, glassy or clear as the bar is, which double-clicking it cycles.
Item {
  id: root

  property string omarchyPath: ""
  property var shell: null
  property var manifest: null

  readonly property bool sidebarOpen: sidebar.opened
  readonly property string barLook: Spaces.barLook(root.shell ? root.shell.barConfig : null)

  function openSidebar() { sidebar.open() }
  function closeSidebar() { sidebar.close() }

  TabStrip {
    look: root.barLook
  }

  Sidebar {
    id: sidebar
    look: root.barLook
    // Closed from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle opens it again.
    onClosed: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "org.ren.renspace")
    }
  }
}
