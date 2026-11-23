pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// The active space's tabs on Omarchy's bar, in place of its workspaces: the
// space's name, which opens the sidebar, then a tab per workspace. Clicks on
// tabs run `space ws`, which stays the only thing that changes workspaces. On
// a vertical bar there is room only for the tabs' numbers.
BarWidget {
  id: root
  moduleName: "org.ren.renspace"

  readonly property var model: tabs.model
  readonly property color foreground: root.bar ? root.bar.barForeground : Commons.Color.bar.text
  readonly property color accent: Commons.Color.accent
  readonly property color hover: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: root.bar ? root.bar.fontFamily : Style.font.family
  readonly property int fontSize: Style.font.body
  readonly property real padding: root.vertical ? 0 : Style.spaceReal(4)

  implicitWidth: root.vertical ? root.barSize : row.implicitWidth + root.padding * 2
  implicitHeight: root.vertical ? column.implicitHeight : root.barSize

  function run(args) {
    Quickshell.execDetached(args)
  }

  function tabLabel(tab) {
    if (root.vertical) return String(tab.slot)
    return tab.slot + "  " + tab.label + (tab.windows > 1 ? "  +" + (tab.windows - 1) : "")
  }

  Tabs {
    id: tabs
  }

  Row {
    id: row
    visible: !root.vertical
    x: root.padding
    height: root.barSize
    spacing: Style.spacing.xs

    // The space's name; click it to show or hide the sidebar.
    TabButton {
      text: tabs.label
      bold: true
      onClicked: root.run(["omarchy-shell", "shell", "call", "org.ren.renspace", "showOrHide", ""])
    }

    Rectangle {
      width: 1
      height: parent.height * 0.5
      anchors.verticalCenter: parent.verticalCenter
      color: root.foreground
      opacity: 0.25
    }

    Repeater {
      model: root.vertical ? [] : root.model.tabs

      TabButton {
        required property var modelData
        text: root.tabLabel(modelData)
        current: modelData.active
        onClicked: root.run(["space", "ws", String(modelData.slot)])
      }
    }

    TabButton {
      text: "+"
      visible: root.model.tabs.length < 10
      onClicked: root.run(["space", "ws", "new"])
    }
  }

  Column {
    id: column
    visible: root.vertical

    Repeater {
      model: root.vertical ? root.model.tabs : []

      TabButton {
        required property var modelData
        text: root.tabLabel(modelData)
        current: modelData.active
        onClicked: root.run(["space", "ws", String(modelData.slot)])
      }
    }
  }

  component TabButton: Item {
    id: button
    property string text: ""
    property bool current: false
    property bool bold: false
    signal clicked()

    width: root.vertical ? root.barSize : Math.min(label.implicitWidth + Style.spacing.rowPaddingX * 2, Style.space(220))
    height: root.vertical ? Style.space(26) : root.barSize

    Rectangle {
      anchors.fill: parent
      anchors.margins: Style.spacing.xxs
      radius: Style.cornerRadius
      color: mouse.containsMouse && !button.current ? root.hover : "transparent"
    }

    Text {
      id: label
      anchors.fill: parent
      anchors.leftMargin: root.vertical ? 0 : Style.spacing.rowPaddingX
      anchors.rightMargin: root.vertical ? 0 : Style.spacing.rowPaddingX
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

    // The current tab is underlined, like a browser's; on a vertical bar,
    // marked on its inner side.
    Rectangle {
      visible: button.current
      x: root.vertical ? (root.bar && root.bar.position === "left" ? parent.width - width : 0) : Style.spacing.sm
      y: root.vertical ? Style.spacing.xs : parent.height - height
      width: root.vertical ? Math.max(2, Style.space(2)) : parent.width - Style.spacing.sm * 2
      height: root.vertical ? parent.height - Style.spacing.xs * 2 : Math.max(2, Style.space(2))
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
