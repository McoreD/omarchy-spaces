pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// What a new space opens, ticked from a list (Controller.qml's
// `settingsOpen`): the gear in the sidebar's header opens it beside the
// sidebar, `settings` through the shell opens it too. None, Terminal, Files,
// then each of Omarchy's agents that is installed, with its own icon. Ticking
// None clears the others, and ticking anything clears None
// (Spaces.toggleOpen); each change is written to the config at once, by `space
// config set`. Made for the keyboard as the pickers are: Up and Down (or Tab,
// Ctrl+N and Ctrl+P) move, Space or Enter ticks, and Escape closes it. Like
// them it has a surface of its own above the windows: a click outside the
// card closes it.
PanelWindow {
  id: root

  property var controller: null
  property var sidebar: null

  readonly property var rows: controller ? controller.newSpaceRows : []
  readonly property var open: controller ? controller.newSpaceOpen : []
  property int current: 0

  readonly property color foreground: Commons.Color.popups.text
  readonly property color accent: Commons.Color.accent
  readonly property var borderSpec: Border.localOrSurfaceSpec("popups", "border", Commons.Color.popups.border, Commons.Color.popups.border, Style.normalBorderWidth)
  readonly property int edge: Style.spacing.md
  readonly property int rowHeight: Math.max(Style.spacing.popupRowHeight, Style.font.body + Style.spacing.controlPaddingY * 2)
  readonly property int iconSize: Math.round(rowHeight * 0.6)
  readonly property int boxSize: Math.round(rowHeight * 0.5)

  visible: !!controller && controller.settingsOpen && !!controller.settingsScreen
  screen: controller ? controller.settingsScreen : null
  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "omarchy-spaces-settings"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

  onVisibleChanged: {
    if (!visible) return
    root.current = 0
    Qt.callLater(function() { keys.forceActiveFocus() })
  }

  onRowsChanged: if (root.current >= root.rows.length) root.current = Math.max(0, root.rows.length - 1)
  onCurrentChanged: list.positionViewAtIndex(root.current, ListView.Contain)

  function move(by) {
    var count = root.rows.length
    if (count > 0) root.current = (root.current + by + count) % count
  }

  function tick(index) {
    var row = root.rows[index]
    if (row && root.controller) root.controller.toggleNewSpace(row.id)
  }

  // Anywhere outside the card closes it.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.AllButtons
    onPressed: root.controller.closeSettings()
  }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.priority: Keys.BeforeItem
    Keys.onPressed: function(event) {
      var ctrl = event.modifiers & Qt.ControlModifier
      if (event.key === Qt.Key_Escape) {
        root.controller.closeSettings()
      } else if (event.key === Qt.Key_Up || event.key === Qt.Key_Backtab || (ctrl && event.key === Qt.Key_P)) {
        root.move(-1)
      } else if (event.key === Qt.Key_Down || event.key === Qt.Key_Tab || (ctrl && event.key === Qt.Key_N)) {
        root.move(1)
      } else if (event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        if (!event.isAutoRepeat) root.tick(root.current)
      } else {
        return
      }
      event.accepted = true
    }
  }

  BorderSurface {
    id: card
    // Beside the sidebar, as far down as the bar.
    x: (root.sidebar ? (root.sidebar.opened ? root.sidebar.panelWidth : root.sidebar.railWidth) : 0) + root.edge
    y: Style.bar.sizeHorizontal + root.edge
    width: Math.min(Style.space(280), root.width - x - root.edge)
    height: content.implicitHeight + contentTopInset + contentBottomInset
    color: Commons.Color.popups.background
    borderSpec: root.borderSpec
    radius: Style.cornerRadius
    padding: Style.spacing.xs

    // Clicks inside stay in the card.
    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
    }

    Column {
      id: content
      x: card.contentLeftInset
      y: card.contentTopInset
      width: card.width - card.contentLeftInset - card.contentRightInset
      spacing: Style.spacing.xxs

      Text {
        width: parent.width
        height: root.rowHeight
        leftPadding: Style.spacing.controlPaddingX
        verticalAlignment: Text.AlignVCenter
        textFormat: Text.PlainText
        text: "A new space opens"
        color: root.foreground
        opacity: 0.5
        font.family: Style.font.family
        font.pixelSize: Style.font.body
      }

      Rectangle {
        width: parent.width - Style.spacing.controlPaddingX * 2
        x: Style.spacing.controlPaddingX
        height: Math.max(1, Style.spacing.hairline)
        color: root.foreground
        opacity: 0.15
      }

      Text {
        visible: !!root.controller && root.controller.settingsError !== ""
        width: parent.width
        leftPadding: Style.spacing.controlPaddingX
        rightPadding: Style.spacing.controlPaddingX
        topPadding: Style.spacing.xxs
        bottomPadding: Style.spacing.xxs
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
        text: root.controller ? root.controller.settingsError : ""
        color: root.foreground
        opacity: 0.8
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }

      ListView {
        id: list
        width: parent.width
        // Scrolls when there are more agents than fit.
        height: Math.min(contentHeight, root.height - card.y - root.edge * 2 - root.rowHeight * 2)
        model: root.rows
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        // The cursor places the view.
        currentIndex: -1

        delegate: Rectangle {
          id: item
          required property var modelData
          required property int index
          readonly property bool hot: index === root.current
          readonly property bool ticked: Spaces.isTicked(root.open, modelData.id)

          width: list.width
          height: root.rowHeight
          radius: Style.cornerRadius
          color: hot ? Style.hoverFillFor(root.foreground, root.accent) : "transparent"

          // The tick box.
          Rectangle {
            id: box
            x: Style.spacing.controlPaddingX
            anchors.verticalCenter: parent.verticalCenter
            width: root.boxSize
            height: root.boxSize
            radius: Math.round(root.boxSize / 4)
            color: item.ticked ? root.accent : "transparent"
            border.width: Math.max(1, Style.spacing.hairline)
            border.color: item.ticked ? root.accent : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.5)

            Text {
              visible: item.ticked
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: "✓"
              color: Commons.Color.popups.background
              font.family: Style.font.family
              font.pixelSize: Math.round(root.boxSize * 0.8)
              font.bold: true
            }
          }

          // Its icon from the icon theme, or its first letter without one.
          Item {
            id: icon
            anchors.left: box.right
            anchors.leftMargin: Style.spacing.controlGap
            anchors.verticalCenter: parent.verticalCenter
            width: item.modelData.id === "none" ? 0 : root.iconSize
            height: root.iconSize

            Image {
              id: image
              anchors.fill: parent
              visible: status === Image.Ready
              // What the icon theme answers is a source as it is, or a path.
              source: {
                var found = item.modelData.icon ? Quickshell.iconPath(item.modelData.icon, true) : ""
                return found.charAt(0) === "/" ? Spaces.fileUrl(found) : found
              }
              fillMode: Image.PreserveAspectFit
              smooth: true
              mipmap: true
              asynchronous: true
              sourceSize.width: root.iconSize * 2
              sourceSize.height: root.iconSize * 2
            }

            Text {
              visible: item.modelData.id !== "none" && image.status !== Image.Ready
              anchors.centerIn: parent
              textFormat: Text.PlainText
              text: item.modelData.name.charAt(0)
              color: root.foreground
              opacity: 0.7
              font.family: Style.font.family
              font.pixelSize: Math.round(root.iconSize * 0.7)
              font.bold: true
            }
          }

          Text {
            anchors.left: icon.right
            anchors.leftMargin: item.modelData.id === "none" ? 0 : Style.spacing.controlGap
            anchors.right: parent.right
            anchors.rightMargin: Style.spacing.controlPaddingX
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: item.modelData.name
            color: item.hot ? Style.hoverStateColor(root.foreground, root.accent) : root.foreground
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
          }

          MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPositionChanged: root.current = item.index
            onClicked: root.tick(item.index)
          }
        }
      }
    }
  }
}
