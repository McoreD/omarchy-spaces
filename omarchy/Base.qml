pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons

// The base the workspaces sit on: the tab strip along the top and the spaces
// sidebar down the left, drawn as one surface per screen on the layer under
// the windows. One surface takes one tint and one blur, so the strip and the
// sidebar read as a single piece with the wallpaper, without a seam where
// they meet. It follows the bar's look: opaque, glassy or clear.
//
// The surface covers the screen but reserves no room and takes clicks only on
// the strip and the open sidebar. The room is reserved by empty panels above
// the windows, one along the top here and the sidebar's (Sidebar.qml), so the
// windows tile around the base.
Scope {
  id: root

  property var tabs: null
  property var sidebar: null
  // The bar's background look: opaque, glassy or none (Spaces.barLook).
  property string look: "opaque"
  // The shell's bar: its edge and its thickness on each screen.
  property var bar: null

  readonly property int stripHeight: Style.bar.sizeHorizontal

  // The strip and the sidebar share the tint, and each takes its text colour
  // from the wallpaper under it when the base is clear.
  BarSurface {
    id: stripSurface
    look: root.look
    edge: "top"
    size: root.stripHeight
  }

  BarSurface {
    id: sidebarSurface
    look: root.look
    edge: "left"
    size: root.sidebar ? root.sidebar.panelWidth : 0
  }

  Variants {
    model: Quickshell.screens

    Scope {
      id: screenBase
      required property var modelData

      // The bar's thickness on this screen, where it runs along `edge`.
      function barOn(edge) {
        if (!root.bar || root.bar.barHidden || root.bar.position !== edge) return 0
        return root.bar.barSizeFor(screenBase.modelData.name)
      }

      // Reserves the strip's room. Hyprland narrows it by the bar beside it,
      // which tells the base where the strip ends. Created before the
      // sidebar's panel, so the sidebar's room starts below the strip.
      PanelWindow {
        id: zone
        screen: screenBase.modelData
        anchors { top: true; left: true; right: true }
        implicitHeight: root.stripHeight
        exclusionMode: ExclusionMode.Normal
        exclusiveZone: root.stripHeight
        mask: Region {}
        color: "transparent"
        WlrLayershell.namespace: "renspace-zone"
        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
      }

      PanelWindow {
        id: base
        screen: screenBase.modelData
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        mask: Region {
          item: strip
          Region { item: sidebarPart.visible ? sidebarPart : null }
        }
        WlrLayershell.namespace: "renspace-base"
        WlrLayershell.layer: WlrLayer.Bottom
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

        readonly property int insetLeft: screenBase.barOn("left")
        readonly property int insetTop: screenBase.barOn("top")
        readonly property int insetBottom: screenBase.barOn("bottom")

        // Drawn first, so while it slides in by itself it passes under the
        // strip's edge rather than over it.
        Rectangle {
          id: sidebarPart
          visible: !!root.sidebar && root.sidebar.shown && root.sidebar.screen === screenBase.modelData
          x: base.insetLeft + (root.sidebar ? root.sidebar.offset : 0)
          y: strip.y + strip.height
          width: root.sidebar ? root.sidebar.panelWidth : 0
          height: base.height - y - base.insetBottom
          color: sidebarSurface.background

          SidebarList {
            anchors.fill: parent
            sidebar: root.sidebar
            foreground: sidebarSurface.foreground
          }
        }

        Rectangle {
          id: strip
          x: base.insetLeft
          y: base.insetTop
          width: zone.width
          height: root.stripHeight
          color: stripSurface.background

          TabStrip {
            anchors.fill: parent
            model: root.tabs ? root.tabs.model : ({ space: "", tabs: [] })
            foreground: stripSurface.foreground
          }
        }
      }
    }
  }
}
