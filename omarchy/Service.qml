import QtQuick
import "Spaces.mjs" as Spaces

// The base the workspaces sit on, loaded with the shell: the tab strip along
// the top and the spaces sidebar on the left, drawn as one surface under the
// windows (Base.qml). The overlay entry (Toggle.qml) opens and closes the
// sidebar so `super+p` goes through the shell like any other plugin.
Item {
  id: root

  property string omarchyPath: ""
  property var shell: null
  property var manifest: null

  readonly property bool sidebarOpen: sidebar.opened

  function openSidebar() { sidebar.open() }
  function closeSidebar() { sidebar.close() }

  Tabs {
    id: tabs
  }

  // Before the sidebar, whose panel then reserves its room below the strip.
  Base {
    tabs: tabs
    sidebar: sidebar
    look: Spaces.barLook(root.shell ? root.shell.barConfig : null)
    bar: root.shell ? root.shell.bar : null
  }

  Sidebar {
    id: sidebar
    // Closed from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle opens it again.
    onClosed: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "org.ren.renspace")
    }
  }
}
