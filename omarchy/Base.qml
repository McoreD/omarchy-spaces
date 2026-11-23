pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

// The spaces sidebar, drawn on the base the workspaces sit on: down the left
// edge, below the bar, on the layer under the windows. It follows the bar's
// look (opaque, glassy or clear), so it reads as one piece with the bar and
// the wallpaper. The surface covers the screen but reserves no room and takes
// clicks only while the sidebar shows; the sidebar's own panel reserves its
// room (Sidebar.qml), so the windows slide aside to uncover it.
Scope {
  id: root

  property var sidebar: null
  // The bar's background look: opaque, glassy or none (Spaces.barLook).
  property string look: "opaque"
  // The shell's bar: its edge and its thickness on each screen.
  property var bar: null

  BarSurface {
    id: surface
    look: root.look
    edge: "left"
    size: root.sidebar ? root.sidebar.panelWidth : 0
  }

  Variants {
    model: Quickshell.screens

    PanelWindow {
      id: base
      required property var modelData
      screen: modelData

      // The bar's thickness on this screen, where it runs along `edge`.
      function barOn(edge) {
        if (!root.bar || root.bar.barHidden || root.bar.position !== edge) return 0
        return root.bar.barSizeFor(base.modelData.name)
      }

      anchors { top: true; bottom: true; left: true; right: true }
      exclusionMode: ExclusionMode.Ignore
      color: "transparent"
      mask: Region { item: sidebarPart.visible ? sidebarPart : null }
      WlrLayershell.namespace: "renspace-base"
      WlrLayershell.layer: WlrLayer.Bottom
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

      Rectangle {
        id: sidebarPart
        visible: !!root.sidebar && root.sidebar.shown && root.sidebar.screen === base.modelData
        x: base.barOn("left") + (root.sidebar ? root.sidebar.offset : 0)
        y: base.barOn("top")
        width: root.sidebar ? root.sidebar.panelWidth : 0
        height: base.height - y - base.barOn("bottom")
        color: surface.background

        // Set back as the background, beside the space's windows; the bar
        // above it is shaded the same (TabsWidget.qml).
        Rectangle {
          anchors.fill: parent
          color: surface.recess
        }

        SidebarList {
          anchors.fill: parent
          sidebar: root.sidebar
          foreground: surface.foreground
        }
      }
    }
  }
}
