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

  readonly property bool searching: sidebarItem.searching
  readonly property var watch: watchItem
  // The sidebar, which the tabs on the bar line up beside while it shows.
  readonly property var sidebar: sidebarItem

  function search() { sidebarItem.search() }
  function endSearch() { sidebarItem.endSearch() }
  function showOrHide(screen) { sidebarItem.showOrHide(screen) }
  function rename() { sidebarItem.renameActive() }
  function renameTab() { sidebarItem.renameTab(0) }

  Base {
    sidebar: sidebarItem
    look: Spaces.barLook(root.shell ? root.shell.barConfig : null)
    bar: root.shell ? root.shell.bar : null
  }

  SpaceMenu {
    sidebar: sidebarItem
  }

  // The spaces, kept current by `space watch`: the sidebar's list, and what
  // the bar calls each space (TabsWidget.qml reaches it through the shell).
  Watch {
    id: watchItem
  }

  // The active space's tabs, for what the one being renamed is called. The
  // bar's widget keeps its own.
  Tabs {
    id: tabsItem
    rows: watchItem.rows
  }

  Sidebar {
    id: sidebarItem
    watch: watchItem
    tabs: tabsItem
    // Ended from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle searches again.
    onSearchEnded: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "org.ren.renspace")
    }
  }
}
