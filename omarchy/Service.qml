import QtQuick

// The frame around the workspaces, loaded with the shell: the tab strip along
// the top, and the spaces sidebar on the left, which the overlay entry
// (Toggle.qml) opens and closes so `super+p` goes through the shell like any
// other plugin. The strip comes first so the sidebar sits below it.
Item {
  id: root

  property string omarchyPath: ""
  property var shell: null
  property var manifest: null

  readonly property bool sidebarOpen: sidebar.opened

  function openSidebar() { sidebar.open() }
  function closeSidebar() { sidebar.close() }

  TabStrip {}

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
