pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import "Spaces.mjs" as Spaces

// The spaces list, part of the frame around the workspaces: the left edge,
// below the tab strip, in the same colour as the strip and the bar. Opening
// it takes room from the windows rather than covering them; Hyprland re-tiles
// them into what's left and animates the move, while the list slides in
// beside them. Picking a space runs `space go`, which stays the only thing
// that knows how spaces map to workspaces.
Item {
  id: root

  // Emitted when the sidebar closes itself (a pick, Escape, focus moving to a
  // window), so the service can tell the shell.
  signal closed()

  property bool opened: false
  property string filterText: ""
  property int selectedIndex: 0
  property var rows: []
  property var shownRows: []
  property string error: ""
  property double openedAt: 0
  readonly property string home: Quickshell.env("HOME") || ""

  readonly property int panelWidth: Style.space(260)
  readonly property color background: Commons.Color.bar.background
  readonly property color foreground: Commons.Color.bar.text
  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int rowHeight: Math.round((Style.font.title + Style.font.bodySmall) * 1.35) + Style.spacing.controlPaddingY * 2
  readonly property int headerHeight: Math.max(Style.space(30), Style.font.subtitle + Style.spacing.controlPaddingY * 2)

  function open() {
    if (root.opened) return
    root.filterText = ""
    root.error = ""
    root.openedAt = Date.now()
    root.opened = true
    root.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    if (!root.opened) return
    root.opened = false
    root.closed()
  }

  function refresh() {
    if (lsProc.running) return
    lsProc.running = true
  }

  function loaded(text, exitCode, stderr) {
    try {
      if (exitCode !== 0) throw new Error(stderr.trim() || "space ls exited with " + exitCode)
      root.rows = Spaces.parseRows(text)
      root.error = ""
    } catch (e) {
      root.rows = []
      root.error = String(e.message || e)
    }
    root.applyFilter(true)
  }

  function applyFilter(resetCursor) {
    root.shownRows = Spaces.filterRows(root.rows, root.filterText)
    if (resetCursor) {
      root.selectedIndex = root.filterText ? 0 : Spaces.initialIndex(root.shownRows)
    }
    if (root.selectedIndex >= root.shownRows.length) root.selectedIndex = Math.max(0, root.shownRows.length - 1)
    Qt.callLater(function() {
      if (root.shownRows.length > 0) list.positionViewAtIndex(root.selectedIndex, ListView.Contain)
    })
  }

  function setFilter(next) {
    root.filterText = next
    root.applyFilter(true)
  }

  function move(delta) {
    var count = root.shownRows.length
    if (count === 0) return
    root.selectedIndex = (root.selectedIndex + delta + count) % count
    list.positionViewAtIndex(root.selectedIndex, ListView.Contain)
  }

  function activate(index) {
    if (index < 0 || index >= root.shownRows.length) return
    var name = root.shownRows[index].name
    root.close()
    Quickshell.execDetached(["space", "go", name])
  }

  function subtitle(row) {
    if (row.dir) return Spaces.displayDir(row.dir, root.home)
    return row.name === "home" ? "Plain workspaces" : "Folder not found"
  }

  function focusedScreen() {
    var monitor = Hyprland.focusedMonitor
    var name = monitor ? String(monitor.name || "") : ""
    for (var i = 0; i < Quickshell.screens.length; i++) {
      if (Quickshell.screens[i].name === name) return Quickshell.screens[i]
    }
    return Quickshell.screens.length > 0 ? Quickshell.screens[0] : null
  }

  Process {
    id: lsProc
    command: ["space", "ls", "--json"]
    property string out: ""
    property string err: ""
    stdout: StdioCollector { onStreamFinished: lsProc.out = text }
    stderr: StdioCollector { onStreamFinished: lsProc.err = text }
    onExited: function(exitCode) {
      // The collectors can finish just after the exit is reported.
      Qt.callLater(function() { root.loaded(lsProc.out, exitCode, lsProc.err) })
    }
  }

  // A click on a window moves focus there; take that as done with the list.
  Connections {
    target: Hyprland
    function onRawEvent(event) {
      if (!root.opened || Date.now() - root.openedAt < 300) return
      if (event.name === "activewindowv2" || event.name === "workspacev2") root.close()
    }
  }

  PanelWindow {
    id: panel

    // Mapped for good and parked just past the screen edge while closed, so
    // opening is a margin change and an animation rather than a new surface.
    screen: root.focusedScreen()
    anchors { left: true; top: true; bottom: true }
    implicitWidth: root.panelWidth
    margins.left: root.opened || slide.running ? 0 : -root.panelWidth
    exclusionMode: ExclusionMode.Normal
    exclusiveZone: root.opened ? root.panelWidth : 0
    color: "transparent"
    WlrLayershell.namespace: "renspace-sidebar"
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.keyboardFocus: root.opened ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    Rectangle {
      id: content
      width: parent.width
      height: parent.height
      x: root.opened ? 0 : -width
      color: root.background

      Behavior on x {
        NumberAnimation { id: slide; duration: 180; easing.type: Easing.OutCubic }
      }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.priority: Keys.BeforeItem
        Keys.onPressed: function(event) {
          var ctrl = event.modifiers & Qt.ControlModifier
          if (event.key === Qt.Key_Escape) {
            if (root.filterText) root.setFilter("")
            else root.close()
          } else if (Util.editsFilter(event, root.filterText)) {
            root.setFilter(Util.editedFilter(event, root.filterText))
          } else if (event.key === Qt.Key_Up || (ctrl && event.key === Qt.Key_P) || event.key === Qt.Key_Backtab) {
            root.move(-1)
          } else if (event.key === Qt.Key_Down || (ctrl && event.key === Qt.Key_N) || event.key === Qt.Key_Tab) {
            root.move(1)
          } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            root.activate(root.selectedIndex)
          } else if (event.text && event.text.length === 1 && !ctrl
              && event.text.charCodeAt(0) >= 32 && event.text.charCodeAt(0) !== 127) {
            root.setFilter(root.filterText + event.text)
          } else {
            return
          }
          event.accepted = true
        }
      }

      Column {
        anchors.fill: parent
        anchors.margins: Style.spacing.md
        spacing: Style.spacing.sm

        Text {
          width: parent.width
          height: root.headerHeight
          leftPadding: Style.spacing.rowPaddingX
          verticalAlignment: Text.AlignVCenter
          textFormat: Text.PlainText
          text: root.filterText || "Spaces"
          color: root.foreground
          opacity: root.filterText ? 1 : 0.5
          font.family: root.fontFamily
          font.pixelSize: Style.font.subtitle
          elide: Text.ElideRight
        }

        Item {
          width: parent.width
          height: parent.height - root.headerHeight - parent.spacing

          ListView {
            id: list
            anchors.fill: parent
            model: root.shownRows
            clip: true
            spacing: Style.spacing.xxs
            boundsBehavior: Flickable.StopAtBounds

            delegate: Rectangle {
              id: rowItem
              required property var modelData
              required property int index
              readonly property bool selected: index === root.selectedIndex

              width: ListView.view.width
              height: root.rowHeight
              radius: Style.cornerRadius
              color: selected ? root.selectedBackground : "transparent"

              // The space you're in.
              Rectangle {
                visible: rowItem.modelData.active
                width: Math.max(2, Style.space(3))
                height: parent.height - Style.spacing.md * 2
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                radius: width / 2
                color: root.accent
              }

              Column {
                anchors.left: parent.left
                anchors.right: windowCount.left
                anchors.leftMargin: Style.spacing.rowPaddingX
                anchors.rightMargin: Style.spacing.controlGap
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.spacing.xxs

                Text {
                  width: parent.width
                  textFormat: Text.PlainText
                  text: rowItem.modelData.name
                  color: rowItem.selected ? root.accent : root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.title
                  font.bold: rowItem.modelData.active
                  elide: Text.ElideRight
                }

                Text {
                  width: parent.width
                  textFormat: Text.PlainText
                  text: root.subtitle(rowItem.modelData)
                  color: root.foreground
                  opacity: 0.5
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  elide: Text.ElideMiddle
                }
              }

              Text {
                id: windowCount
                anchors.right: parent.right
                anchors.rightMargin: Style.spacing.rowPaddingX
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: rowItem.modelData.windows >= 0 ? String(rowItem.modelData.windows) : ""
                color: root.foreground
                opacity: rowItem.modelData.windows > 0 ? 0.7 : 0.35
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
              }

              MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onContainsMouseChanged: if (containsMouse) root.selectedIndex = rowItem.index
                onClicked: root.activate(rowItem.index)
              }
            }
          }

          Text {
            anchors.centerIn: parent
            width: parent.width
            visible: root.shownRows.length === 0 && (root.error !== "" || root.filterText !== "")
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            text: root.error !== "" ? root.error : "No space matches “" + root.filterText + "”"
            color: root.foreground
            opacity: 0.7
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
          }
        }
      }
    }
  }
}
