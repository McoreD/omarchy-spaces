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
  // Shared interaction and theme state for the bar, sidebar, and popups.
  readonly property var controller: controllerItem

  // The tabs' widgets with the sidebar's header on their bar, or null until
  // one has said whether it has.
  property var headerBars: null
  function setHeader(widget, on) {
    var bars = (root.headerBars || []).filter(function(bar) { return bar && bar !== widget })
    if (on) bars.push(widget)
    root.headerBars = bars
  }

  function search() { sidebarItem.search() }
  function endSearch() { sidebarItem.endSearch() }
  function toggleSidebar(screen) { sidebarItem.toggleSidebar(screen) }
  function renameSpace() { controllerItem.renameSpace() }
  function renameTab() { controllerItem.renameTab("") }
  function sendTab() { controllerItem.sendTab() }

  // What the sidebar and the bars show, for `just stress` to check.
  function shown() {
    return JSON.stringify({
      space: sidebarItem.shownRows.filter(function(row) { return row.active }).map(function(row) { return row.id }),
      focused: watchItem.now.focused,
      monitors: watchItem.now.monitors.map(function(m) { return { name: m.name, space: m.space, tab: m.tab } }),
    })
  }

  Base {
    sidebar: sidebarItem
    controller: controllerItem
    look: Spaces.barLook(root.shell ? root.shell.barConfig : null)
    bar: root.shell ? root.shell.bar : null
  }

  SpaceMenu {
    controller: controllerItem
  }

  MovePicker {
    controller: controllerItem
  }

  IconPicker {
    controller: controllerItem
    omarchyPath: root.omarchyPath
  }

  // What the sidebar and the bar show, kept current by `space watch`
  // (TabsWidget.qml reaches it through the shell).
  Watch {
    id: watchItem
  }

  Controller {
    id: controllerItem
    watch: watchItem
    sidebar: sidebarItem
  }

  // The spaces from before the last restart open again by themselves, unless
  // `[restore]` sets `enabled = false`. It does nothing when they already
  // have, as when only the shell restarts; detached, so restarting the shell
  // doesn't stop it halfway.
  Component.onCompleted: Quickshell.execDetached(["space", "restore", "--auto"])

  Sidebar {
    id: sidebarItem
    watch: watchItem
    controller: controllerItem
    // As the tabs' widgets have it, or as the config does until they load,
    // so the list doesn't start with a header that then goes.
    headerOnBar: root.headerBars ? root.headerBars.length > 0
      : Spaces.headerOnBar(root.shell ? root.shell.barConfig : null, (root.manifest && root.manifest.id) || "com.t4t5.spaces")
    // Ended from inside (a pick, Escape), so the shell stops counting the
    // overlay as open and the next toggle searches again.
    onSearchEnded: {
      if (root.shell && typeof root.shell.hide === "function")
        root.shell.hide((root.manifest && root.manifest.id) || "com.t4t5.spaces")
    }
  }
}
