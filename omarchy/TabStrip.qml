pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import qs.Commons
import qs.Commons as Commons

// The active space's tabs (Tabs.qml), drawn on the base along the top edge.
// Clicking the space's name opens the sidebar; clicks on tabs run `space ws`,
// which stays the only thing that changes workspaces.
Item {
  id: root

  property var model: ({ space: "", tabs: [] })
  property color foreground: Commons.Color.bar.text

  readonly property color accent: Commons.Color.accent
  readonly property color hover: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int fontSize: Style.font.body

  function run(args) {
    Quickshell.execDetached(args)
  }

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

  component StripButton: Item {
    id: button
    property string text: ""
    property bool current: false
    property bool bold: false
    signal clicked()

    width: Math.min(label.implicitWidth + Style.spacing.rowPaddingX * 2, Style.space(220))
    height: root.height

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
