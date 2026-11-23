import QtQuick
import "Spaces.mjs" as Spaces

// The spaces sidebar, loaded with the shell and drawn under the windows
// (Base.qml). The overlay entry (Toggle.qml) starts and ends its search, so
// `super+p` goes through the shell like any other plugin. The tabs are a bar widget
// (TabsWidget.qml), which the bar loads itself.
Item {
  id: root

  property string omarchyPath: ""
  property var shell: null
  property var manifest: null

  readonly property bool searching: sidebar.searching

  function search() { sidebar.search() }
  function endSearch() { sidebar.endSearch() }
  function showOrHide() { sidebar.showOrHide() }

  Base {
    sidebar: sidebar
    look: Spaces.barLook(root.shell ? root.shell.barConfig : null)
    bar: root.shell ? root.shell.bar : null
  }

  Sidebar {
    id: sidebar
    // Ended from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle searches again.
    onSearchEnded: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "org.ren.renspace")
    }
  }
}
