import QtQuick
import Quickshell
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
  function toggleSidebar(screen) { sidebarItem.toggleSidebar(screen) }
  function renameSpace() { sidebarItem.renameSpace() }
  function renameTab() { sidebarItem.renameTab(0) }
  function sendTab() { sidebarItem.sendTab() }

  // What the sidebar and the bars show, for `just stress` to check.
  function shown() {
    return JSON.stringify({
      space: sidebarItem.shownRows.filter(function(row) { return row.active }).map(function(row) { return row.id }),
      focused: watchItem.now.focused,
      monitors: watchItem.now.monitors.map(function(m) { return { name: m.name, space: m.space, key: m.key } }),
    })
  }

  Base {
    sidebar: sidebarItem
    look: Spaces.barLook(root.shell ? root.shell.barConfig : null)
    bar: root.shell ? root.shell.bar : null
  }

  SpaceMenu {
    sidebar: sidebarItem
  }

  MovePicker {
    sidebar: sidebarItem
  }

  IconPicker {
    sidebar: sidebarItem
    omarchyPath: root.omarchyPath
  }

  // What the sidebar and the bar show, kept current by `space watch`
  // (TabsWidget.qml reaches it through the shell).
  Watch {
    id: watchItem
  }

  // The spaces from before the last restart open again by themselves. It does
  // nothing when they already have, as when only the shell restarts; detached,
  // so restarting the shell doesn't stop it halfway.
  Component.onCompleted: Quickshell.execDetached(["space", "restore"])

  Sidebar {
    id: sidebarItem
    watch: watchItem
    headerOnBar: Spaces.barToggle(root.shell ? root.shell.barConfig : null, (root.manifest && root.manifest.id) || "com.t4t5.spaces")
    // Ended from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle searches again.
    onSearchEnded: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "com.t4t5.spaces")
    }
  }
}
