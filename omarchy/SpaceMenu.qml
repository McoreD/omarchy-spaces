pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// A space's menu, floating where you right-clicked it in the sidebar: rename
// it, close its windows, or remove it (Spaces.rowActions). The sidebar is
// drawn under the windows, so the menu gets a surface of its own above them,
// covering the screen while it is open: a click anywhere outside the card
// closes it, as does Escape. Up and Down (or j and k) move through it, and
// Enter picks.
PanelWindow {
  id: root

  property var sidebar: null

  readonly property var row: sidebar ? sidebar.menuRow : null
  readonly property var actions: Spaces.rowActions(row)
  property int current: -1

  readonly property color foreground: Commons.Color.popups.text
  readonly property color accent: Commons.Color.accent
  readonly property var borderSpec: Border.localOrSurfaceSpec("popups", "border", Commons.Color.popups.border, Commons.Color.popups.border, Style.normalBorderWidth)
  readonly property int itemHeight: Math.max(Style.spacing.popupRowHeight, Style.font.body + Style.spacing.controlPaddingY * 2)
  readonly property int edge: Style.spacing.md

  visible: actions.length > 0 && !!sidebar && !!sidebar.menuScreen
  screen: sidebar ? sidebar.menuScreen : null
  anchors { top: true; bottom: true; left: true; right: true }
  exclusionMode: ExclusionMode.Ignore
  color: "transparent"
  WlrLayershell.namespace: "omarchy-spaces-menu"
  WlrLayershell.layer: WlrLayer.Overlay
  WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

  onVisibleChanged: {
    root.current = -1
    if (visible) Qt.callLater(function() { keys.forceActiveFocus() })
  }

  function pick(index) {
    var action = root.actions[index]
    if (action && action.enabled) root.sidebar.act(action.id, root.row)
  }

  // Anywhere outside the card closes the menu.
  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.AllButtons
    onPressed: root.sidebar.closeMenu()
  }

  Item {
    id: keys
    anchors.fill: parent
    focus: true
    Keys.onPressed: function(event) {
      if (event.key === Qt.Key_Escape) root.sidebar.closeMenu()
      else if (event.key === Qt.Key_Down || event.text === "j" || event.key === Qt.Key_Tab) root.current = Spaces.stepAction(root.actions, root.current, 1)
      else if (event.key === Qt.Key_Up || event.text === "k" || event.key === Qt.Key_Backtab) root.current = Spaces.stepAction(root.actions, root.current, -1)
      else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) root.pick(root.current)
      else return
      event.accepted = true
    }
  }

  BorderSurface {
    id: card
    // Where you clicked, kept on the screen: above the pointer when there
    // isn't room below it.
    x: Math.max(root.edge, Math.min(root.sidebar ? root.sidebar.menuX : 0, root.width - width - root.edge))
    y: {
      var at = root.sidebar ? root.sidebar.menuY : 0
      return at + height + root.edge <= root.height ? at : Math.max(root.edge, at - height)
    }
    width: Style.space(240)
    height: items.implicitHeight + contentTopInset + contentBottomInset
    color: Commons.Color.popups.background
    borderSpec: root.borderSpec
    radius: Style.cornerRadius
    padding: Style.spacing.xs

    // Clicks between the items stay in the card.
    MouseArea {
      anchors.fill: parent
      acceptedButtons: Qt.AllButtons
    }

    Column {
      id: items
      x: card.contentLeftInset
      y: card.contentTopInset
      width: card.width - card.contentLeftInset - card.contentRightInset
      spacing: Style.spacing.xxs

      Repeater {
        model: root.actions

        delegate: Column {
          id: item
          required property var modelData
          required property int index
          readonly property bool hot: index === root.current && modelData.enabled

          width: items.width
          spacing: Style.spacing.xxs

          Rectangle {
            visible: item.modelData.separatorBefore === true
            width: parent.width - Style.spacing.controlPaddingX * 2
            x: Style.spacing.controlPaddingX
            height: Math.max(1, Style.spacing.hairline)
            color: root.foreground
            opacity: 0.15
          }

          Item {
            visible: item.modelData.separatorBefore === true
            width: 1
            height: Style.spacing.xxs
          }

          Rectangle {
            width: parent.width
            height: root.itemHeight
            radius: Style.cornerRadius
            color: item.hot ? Style.hoverFillFor(root.foreground, root.accent) : "transparent"
            opacity: item.modelData.enabled ? 1 : 0.4

            Text {
              id: icon
              anchors.left: parent.left
              anchors.leftMargin: Style.spacing.controlPaddingX
              anchors.verticalCenter: parent.verticalCenter
              width: Style.font.icon
              horizontalAlignment: Text.AlignHCenter
              textFormat: Text.PlainText
              text: item.modelData.icon
              color: item.hot ? Style.hoverStateColor(root.foreground, root.accent) : root.foreground
              font.family: Style.font.family
              font.pixelSize: Style.font.icon
            }

            Text {
              anchors.left: icon.right
              anchors.right: hint.left
              anchors.leftMargin: Style.spacing.controlGap
              anchors.rightMargin: Style.spacing.controlGap
              anchors.verticalCenter: parent.verticalCenter
              textFormat: Text.PlainText
              text: item.modelData.label
              color: item.hot ? Style.hoverStateColor(root.foreground, root.accent) : root.foreground
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              elide: Text.ElideRight
            }

            Text {
              id: hint
              anchors.right: parent.right
              anchors.rightMargin: Style.spacing.controlPaddingX
              anchors.verticalCenter: parent.verticalCenter
              textFormat: Text.PlainText
              text: item.modelData.hint
              color: root.foreground
              opacity: 0.5
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: item.modelData.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
              onPositionChanged: root.current = item.modelData.enabled ? item.index : -1
              onExited: if (root.current === item.index) root.current = -1
              onClicked: root.pick(item.index)
            }
          }
        }
      }
    }
  }
}
