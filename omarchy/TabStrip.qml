pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import "Spaces.mjs" as Spaces

// A strip along the top edge with the active space's tabs, which are its
// workspaces. Clicking the space's name opens the sidebar. Read live from Hyprland, so it needs no `space` call to stay
// current; clicks run `space ws`, which stays the only thing that changes
// workspaces.
Item {
  id: root

  // Bumped after Hyprland has answered a refresh, for anything bindings miss.
  property int revision: 0

  readonly property var model: {
    root.revision
    var active = Hyprland.focusedWorkspace
    var workspaces = Hyprland.workspaces.values.map(function(ws) { return { id: ws.id, name: ws.name } })
    var windows = []
    var toplevels = Hyprland.toplevels.values
    for (var i = 0; i < toplevels.length; i++) {
      var ipc = toplevels[i].lastIpcObject || {}
      var workspace = toplevels[i].workspace
      windows.push({
        workspaceId: workspace ? workspace.id : (ipc.workspace ? ipc.workspace.id : 0),
        windowClass: ipc.class || "",
        focus: typeof ipc.focusHistoryID === "number" ? ipc.focusHistoryID : 1e9,
      })
    }
    return Spaces.tabsFor(workspaces, windows, active ? { id: active.id, name: active.name } : null)
  }

  // The bar's background look: opaque, glassy or none (Spaces.barLook).
  property string look: "opaque"

  readonly property int stripHeight: Style.bar.sizeHorizontal
  readonly property color background: surface.background
  readonly property color foreground: surface.foreground
  readonly property color accent: Commons.Color.accent
  readonly property color hover: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int fontSize: Style.font.body

  BarSurface {
    id: surface
    look: root.look
    edge: "top"
    size: root.stripHeight
  }

  function run(args) {
    Quickshell.execDetached(args)
  }

  // Window classes and focus order come from Hyprland's IPC, which Quickshell
  // only asks for on request. Ask after the events that change them, once a
  // burst has settled.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      switch (event.name) {
      case "openwindow": case "closewindow": case "movewindowv2": case "activewindowv2":
      case "workspacev2": case "createworkspacev2": case "destroyworkspacev2":
      case "renameworkspace": case "focusedmonv2":
        refresh.restart()
      }
    }
  }

  Timer {
    id: refresh
    interval: 30
    onTriggered: {
      Hyprland.refreshWorkspaces()
      Hyprland.refreshToplevels()
      settle.restart()
    }
  }

  Timer {
    id: settle
    interval: 120
    onTriggered: root.revision++
  }

  Component.onCompleted: refresh.restart()

  Variants {
    model: Quickshell.screens

    PanelWindow {
      id: strip
      required property var modelData
      screen: modelData

      anchors { top: true; left: true; right: true }
      implicitHeight: root.stripHeight
      exclusionMode: ExclusionMode.Normal
      exclusiveZone: root.stripHeight
      color: root.background
      WlrLayershell.namespace: "renspace-tabs"
      WlrLayershell.layer: WlrLayer.Top
      WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

      Row {
        anchors.left: parent.left
        anchors.leftMargin: Style.spacing.md
        height: parent.height
        spacing: Style.spacing.xs

        // The space's name; click it to switch space.
        StripButton {
          text: root.model.space
          bold: true
          onClicked: root.run(["omarchy-shell", "shell", "toggle", "org.ren.renspace", ""])
        }

        Rectangle {
          width: 1
          height: parent.height * 0.5
          anchors.verticalCenter: parent.verticalCenter
          color: root.foreground
          opacity: 0.25
        }

        Repeater {
          model: root.model.tabs

          StripButton {
            required property var modelData
            text: modelData.slot + "  " + modelData.label + (modelData.windows > 1 ? "  +" + (modelData.windows - 1) : "")
            current: modelData.active
            onClicked: root.run(["space", "ws", String(modelData.slot)])
          }
        }

        StripButton {
          text: "+"
          visible: root.model.tabs.length < 10
          onClicked: root.run(["space", "ws", "new"])
        }
      }
    }
  }

  component StripButton: Item {
    id: button
    property string text: ""
    property bool current: false
    property bool bold: false
    signal clicked()

    width: Math.min(label.implicitWidth + Style.spacing.rowPaddingX * 2, Style.space(220))
    height: root.stripHeight

    Rectangle {
      anchors.fill: parent
      anchors.topMargin: Style.spacing.xxs
      anchors.bottomMargin: Style.spacing.xxs
      radius: Style.cornerRadius
      color: mouse.containsMouse && !button.current ? root.hover : "transparent"
    }

    Text {
      id: label
      anchors.fill: parent
      anchors.leftMargin: Style.spacing.rowPaddingX
      anchors.rightMargin: Style.spacing.rowPaddingX
      verticalAlignment: Text.AlignVCenter
      horizontalAlignment: Text.AlignHCenter
      textFormat: Text.PlainText
      text: button.text
      elide: Text.ElideRight
      color: button.current ? root.accent : root.foreground
      opacity: button.current || button.bold ? 1 : 0.6
      font.family: root.fontFamily
      font.pixelSize: root.fontSize
      font.bold: button.bold || button.current
    }

    // The current tab is underlined, like a browser's.
    Rectangle {
      visible: button.current
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.bottom: parent.bottom
      anchors.leftMargin: Style.spacing.sm
      anchors.rightMargin: Style.spacing.sm
      height: Math.max(2, Style.space(2))
      color: root.accent
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: button.clicked()
    }
  }
}
