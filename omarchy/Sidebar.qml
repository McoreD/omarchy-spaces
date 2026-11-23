pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// A column on the left edge listing every space. Summoned and dismissed like
// Omarchy's other overlays; picking a space runs `space go`, which stays the
// only thing that knows how spaces map to workspaces.
Item {
  id: root

  property string omarchyPath: Quickshell.env("OMARCHY_PATH")
  property var shell: null
  property var manifest: null

  property bool opened: false
  property string filterText: ""
  property int selectedIndex: 0
  property var rows: []
  property var shownRows: []
  property string error: ""
  readonly property string home: Quickshell.env("HOME") || ""

  // The menu surface tokens, so themes that style Omarchy's menus style this.
  property color background: Commons.Color.menu.background
  property color foreground: Commons.Color.menu.text
  property color border: Commons.Color.menu.border
  property var borderSpec: Border.surfaceSpec("menu", "border", border, Math.max(1, Style.space(2)))
  property color scrim: Commons.Color.menu.scrim
  property color selectedBackground: Commons.Color.menu.selectedBackground
  property color selectedText: Commons.Color.menu.selectedText
  property string fontFamily: Style.font.menuFamily
  readonly property int cornerRadius: Style.cornerRadius
  readonly property int contentMargin: Style.spacing.panelPadding
  readonly property int headerHeight: Math.max(Style.space(34), Style.font.heading + Style.spacing.controlPaddingY * 2)
  readonly property int rowHeight: Math.round((Style.font.title + Style.font.bodySmall) * 1.35) + Style.spacing.controlPaddingY * 2
  readonly property int cardWidth: Math.min(Style.space(300), panel.width - Style.gapsOut * 2)

  function open(payloadJson) {
    root.opened = true
    root.filterText = ""
    root.error = ""
    root.refresh()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  function close() {
    root.opened = false
  }

  function dismiss() {
    root.opened = false
    if (root.shell && typeof root.shell.hide === "function")
      root.shell.hide((root.manifest && root.manifest.id) || "org.ren.renspace")
  }

  function toggle() {
    if (root.opened) root.dismiss()
    else root.open("{}")
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
    root.dismiss()
    Quickshell.execDetached(["space", "go", name])
  }

  function subtitle(row) {
    if (row.dir) return Spaces.displayDir(row.dir, root.home)
    return row.name === "home" ? "Plain workspaces" : "Folder not found"
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

  OverlayWindow {
    id: panel
    shown: root.opened
    WlrLayershell.namespace: "renspace-sidebar"

    Rectangle {
      anchors.fill: parent
      color: root.scrim
    }

    MouseArea {
      anchors.fill: parent
      onClicked: root.dismiss()
    }

    BorderSurface {
      id: card
      width: root.cardWidth
      anchors.left: parent.left
      anchors.top: parent.top
      anchors.bottom: parent.bottom
      anchors.margins: Style.gapsOut
      radius: root.cornerRadius
      color: root.background
      borderSpec: root.borderSpec
      padding: root.contentMargin

      MouseArea { anchors.fill: parent; onClicked: {} }

      Item {
        id: keyCatcher
        anchors.fill: parent
        focus: true

        Keys.priority: Keys.BeforeItem
        Keys.onPressed: function(event) {
          var ctrl = event.modifiers & Qt.ControlModifier
          if (event.key === Qt.Key_Escape) {
            if (root.filterText) root.setFilter("")
            else root.dismiss()
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
        anchors.topMargin: card.contentTopInset
        anchors.rightMargin: card.contentRightInset
        anchors.bottomMargin: card.contentBottomInset
        anchors.leftMargin: card.contentLeftInset
        spacing: Style.spacing.md

        Text {
          width: parent.width
          height: root.headerHeight
          verticalAlignment: Text.AlignVCenter
          textFormat: Text.PlainText
          text: root.filterText || "Spaces"
          color: root.foreground
          opacity: root.filterText ? 1 : 0.58
          font.family: root.fontFamily
          font.pixelSize: Style.font.heading
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
              radius: root.cornerRadius
              color: selected ? root.selectedBackground : "transparent"

              // The space you're in.
              Rectangle {
                visible: rowItem.modelData.active
                width: Math.max(2, Style.space(3))
                height: parent.height - Style.spacing.md * 2
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                radius: width / 2
                color: root.selectedText
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
                  color: rowItem.selected ? root.selectedText : root.foreground
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
                  opacity: 0.55
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
