pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// The Settings screen (Controller.qml's `settingsOpen`): the gear in the
// sidebar's header opens it beside the sidebar, `settings` through the shell
// opens it too. It is drawn from what `space config schema --json` lists
// (Spaces.parseSchema, Spaces.settingsItems), so a setting that command knows
// needs nothing here: a switch is a tick box, a choice a list of radio rows,
// a setting of several a list of tick rows (None clears the others, and
// ticking anything clears None, Spaces.toggled). Each change is written to the
// config at once, by `space config set`, and a write that fails says why under
// its setting. Made for the keyboard as the pickers are: Up and Down (or Tab,
// Ctrl+N and Ctrl+P) move through the rows that can be changed, Space or Enter
// changes one, and Escape closes it. Like them it has a surface of its own
// above the windows: a click outside the card closes it.
PanelWindow {
  id: root

  property var controller: null
  property var sidebar: null

  readonly property var items: controller ? controller.settingsItems : []
  readonly property var values: controller ? controller.settingsValues : ({})
  readonly property var errors: controller ? controller.settingsErrors : ({})
  // The item the cursor is on, in `items`, and for a grid the cell in it.
  property int current: -1
  property int cell: 0
  // How many cells of a grid fit in a row, as wide as the list.
  readonly property int cellMin: Math.round(rowHeight * 2.0)
  readonly property int gridCols: Spaces.gridColumns(list.width - Style.spacing.controlPaddingX, root.cellMin)
  // A name takes up to two lines, in a slightly smaller font.
  readonly property int cellFont: Math.round(Style.font.bodySmall * 0.85)
  readonly property int cellHeight: iconSize + cellFont * 2 + Style.spacing.xs * 2 + Style.spacing.xxs * 2

  readonly property color foreground: Commons.Color.popups.text
  readonly property color accent: Commons.Color.accent
  readonly property var borderSpec: Border.localOrSurfaceSpec("popups", "border", Commons.Color.popups.border, Commons.Color.popups.border, Style.normalBorderWidth)
  readonly property int edge: Style.spacing.md
  readonly property int rowHeight: Math.max(Style.spacing.popupRowHeight, Style.font.body + Style.spacing.controlPaddingY * 2)
  readonly property int sectionHeight: Math.round(rowHeight * 0.8)
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
    root.current = Spaces.firstFocus(root.items)
    root.cell = 0
    Qt.callLater(function() { keys.forceActiveFocus() })
  }

  // The cursor stays where it is when the rows are read again, if it still
  // can; else it goes to the first row.
  onItemsChanged: {
    if (root.current < 0 || root.current >= root.items.length || !Spaces.canFocus(root.items[root.current])) {
      root.current = Spaces.firstFocus(root.items)
      root.cell = 0
    } else if (Spaces.optionAt(root.items, { index: root.current, cell: root.cell }) === null) {
      root.cell = 0
    }
  }

  onCurrentChanged: {
    if (root.current < 0) return
    // Going up, the section and the heading come into view with the row.
    list.positionViewAtIndex(Spaces.leadIn(root.items, root.current), ListView.Contain)
    list.positionViewAtIndex(root.current, ListView.Contain)
  }

  // Moves the cursor one step: "up", "down", "left" or "right".
  function step(dir) {
    var next = Spaces.moveCursor(root.items, { index: root.current, cell: root.cell }, dir, root.gridCols)
    root.cell = next.cell
    root.current = next.index
  }

  function place(index, cell) {
    root.cell = cell
    root.current = index
  }

  function change(index, cell) {
    var option = Spaces.optionAt(root.items, { index: index, cell: cell })
    if (option && root.controller) root.controller.changeSetting(option.key, option.value)
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
        root.step("up")
      } else if (event.key === Qt.Key_Down || event.key === Qt.Key_Tab || (ctrl && event.key === Qt.Key_N)) {
        root.step("down")
      } else if (event.key === Qt.Key_Left) {
        root.step("left")
      } else if (event.key === Qt.Key_Right) {
        root.step("right")
      } else if (event.key === Qt.Key_Space || event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
        if (!event.isAutoRepeat) root.change(root.current, root.cell)
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
    width: Math.min(Style.space(320), root.width - x - root.edge)
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
        text: "Settings"
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
        visible: !!root.controller && root.controller.settingsReadError !== ""
        width: parent.width
        leftPadding: Style.spacing.controlPaddingX
        rightPadding: Style.spacing.controlPaddingX
        topPadding: Style.spacing.xxs
        bottomPadding: Style.spacing.xxs
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
        text: root.controller ? root.controller.settingsReadError : ""
        color: root.foreground
        opacity: 0.8
        font.family: Style.font.family
        font.pixelSize: Style.font.bodySmall
      }

      ListView {
        id: list
        width: parent.width
        // Scrolls when there are more settings than fit.
        height: Math.min(contentHeight, root.height - card.y - root.edge * 2 - root.rowHeight * 2)
        model: root.items
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        // Every row is made, so the height is the real one.
        cacheBuffer: 100000
        // The cursor places the view.
        currentIndex: -1

        delegate: Item {
          id: item
          required property var modelData
          required property int index
          readonly property string kind: modelData.kind
          readonly property bool isOption: kind === "option"
          readonly property bool isGrid: kind === "grid"
          readonly property bool hot: isOption && index === root.current
          readonly property bool on: isOption && Spaces.isOn(modelData, root.values)
          readonly property bool radio: isOption && modelData.type === "choice"
          readonly property bool hasIcon: isOption && modelData.icon !== ""
          readonly property string error: modelData.key ? (root.errors[modelData.key] || modelData.problem || "") : ""

          width: list.width
          height: kind === "section" ? root.sectionHeight + (index > 0 ? Style.spacing.md : 0)
            : kind === "subheading" ? root.sectionHeight
            : kind === "grid" ? Math.ceil(modelData.cells.length / root.gridCols) * root.cellHeight + root.sectionHeight
            : kind === "heading" ? heading.implicitHeight
            : Math.max(root.rowHeight, optionText.implicitHeight + Style.spacing.xxs * 2)

          // A section's title.
          Text {
            id: sectionTitle
            visible: item.kind === "section"
            anchors.bottom: parent.bottom
            width: parent.width
            height: root.sectionHeight
            leftPadding: Style.spacing.controlPaddingX
            verticalAlignment: Text.AlignVCenter
            textFormat: Text.PlainText
            text: item.kind === "section" ? item.modelData.title : ""
            color: root.accent
            opacity: 0.9
            font.family: Style.font.family
            font.pixelSize: Style.font.bodySmall
            font.bold: true
          }

          // A group's name, over its options.
          Text {
            visible: item.kind === "subheading"
            anchors.bottom: parent.bottom
            width: parent.width
            height: root.sectionHeight
            leftPadding: Style.spacing.controlPaddingX
            verticalAlignment: Text.AlignVCenter
            textFormat: Text.PlainText
            text: item.kind === "subheading" ? item.modelData.name : ""
            color: root.foreground
            opacity: 0.6
            font.family: Style.font.family
            font.pixelSize: Style.font.bodySmall
          }

          // A group drawn as a grid: each cell an icon over a short name, lit
          // when it is on, and under it the full name of the cell the cursor
          // is on.
          Item {
            visible: item.isGrid
            anchors.fill: parent

            Repeater {
              model: item.isGrid ? item.modelData.cells : []

              delegate: Rectangle {
                id: cellItem
                required property var modelData
                required property int index
                readonly property bool hot: item.index === root.current && index === root.cell
                readonly property bool on: Spaces.isOn(modelData, root.values)
                readonly property real cellWidth: Math.floor((list.width - Style.spacing.controlPaddingX) / root.gridCols)

                x: Style.spacing.controlPaddingX / 2 + (index % root.gridCols) * cellWidth
                y: Math.floor(index / root.gridCols) * root.cellHeight
                width: cellWidth - Style.spacing.xxs
                height: root.cellHeight - Style.spacing.xxs
                radius: Style.cornerRadius
                color: on ? Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.28) : (hot ? Style.hoverFillFor(root.foreground, root.accent) : "transparent")
                // Lit when it is on: the accent. The cursor is a ring of its own,
                // inside, so a cell that is both shows both.
                border.width: on ? Math.max(1, Style.spacing.hairline) : 0
                border.color: root.accent

                Rectangle {
                  visible: cellItem.hot
                  anchors.fill: parent
                  anchors.margins: Math.max(2, Style.spacing.hairline * 2)
                  radius: Math.max(1, Style.cornerRadius - 2)
                  color: "transparent"
                  border.width: Math.max(1, Style.spacing.hairline)
                  border.color: root.foreground
                }

                Item {
                  id: cellIcon
                  anchors.horizontalCenter: parent.horizontalCenter
                  y: Style.spacing.xs
                  width: root.iconSize
                  height: root.iconSize

                  Image {
                    id: cellImage
                    anchors.fill: parent
                    visible: status === Image.Ready
                    source: {
                      var found = cellItem.modelData.icon ? Quickshell.iconPath(cellItem.modelData.icon, true) : ""
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
                    visible: cellImage.status !== Image.Ready
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: cellItem.modelData.name.charAt(0)
                    color: root.foreground
                    opacity: 0.7
                    font.family: Style.font.family
                    font.pixelSize: Math.round(root.iconSize * 0.7)
                    font.bold: true
                  }
                }

                Text {
                  anchors.top: cellIcon.bottom
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.leftMargin: Style.spacing.xxs
                  anchors.rightMargin: Style.spacing.xxs
                  horizontalAlignment: Text.AlignHCenter
                  textFormat: Text.PlainText
                  text: cellItem.modelData.label
                  color: root.foreground
                  font.family: Style.font.family
                  font.pixelSize: root.cellFont
                  wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                  maximumLineCount: 2
                  elide: Text.ElideRight
                }

                MouseArea {
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onPositionChanged: root.place(item.index, cellItem.index)
                  onClicked: root.change(item.index, cellItem.index)
                }
              }
            }

            // The full name of the cell the cursor is on.
            Text {
              anchors.left: parent.left
              anchors.right: parent.right
              anchors.bottom: parent.bottom
              height: root.sectionHeight
              leftPadding: Style.spacing.controlPaddingX
              verticalAlignment: Text.AlignVCenter
              textFormat: Text.PlainText
              text: {
                if (!item.isGrid || item.index !== root.current) return ""
                var option = Spaces.optionAt(root.items, { index: item.index, cell: root.cell })
                return option ? option.name : ""
              }
              color: root.foreground
              opacity: 0.6
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
              elide: Text.ElideRight
            }
          }

          // The name of a choice or a setting of several, and why a write of
          // it failed.
          Column {
            id: heading
            visible: item.kind === "heading"
            width: parent.width
            topPadding: Style.spacing.xxs

            Text {
              width: parent.width
              leftPadding: Style.spacing.controlPaddingX
              rightPadding: Style.spacing.controlPaddingX
              wrapMode: Text.Wrap
              textFormat: Text.PlainText
              text: item.kind === "heading" ? item.modelData.name : ""
              color: root.foreground
              opacity: 0.8
              font.family: Style.font.family
              font.pixelSize: Style.font.body
            }

            Text {
              visible: text !== ""
              width: parent.width
              leftPadding: Style.spacing.controlPaddingX
              rightPadding: Style.spacing.controlPaddingX
              wrapMode: Text.Wrap
              textFormat: Text.PlainText
              text: item.kind === "heading" ? item.modelData.help : ""
              color: root.foreground
              opacity: 0.5
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
            }

            Text {
              visible: text !== "" && item.kind === "heading"
              width: parent.width
              leftPadding: Style.spacing.controlPaddingX
              rightPadding: Style.spacing.controlPaddingX
              wrapMode: Text.Wrap
              textFormat: Text.PlainText
              text: item.kind === "heading" ? item.error : ""
              color: root.foreground
              opacity: 0.8
              font.family: Style.font.family
              font.pixelSize: Style.font.bodySmall
            }
          }

          // A row that can be changed.
          Rectangle {
            visible: item.isOption
            anchors.fill: parent
            radius: Style.cornerRadius
            color: item.hot ? Style.hoverFillFor(root.foreground, root.accent) : "transparent"

            // The tick box, or the radio of a choice.
            Rectangle {
              id: box
              x: Style.spacing.controlPaddingX
              anchors.verticalCenter: parent.verticalCenter
              width: root.boxSize
              height: root.boxSize
              radius: item.radio ? Math.round(root.boxSize / 2) : Math.round(root.boxSize / 4)
              color: item.on && !item.radio ? root.accent : "transparent"
              border.width: Math.max(1, Style.spacing.hairline)
              border.color: item.on ? root.accent : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.5)

              Text {
                visible: item.on && !item.radio
                anchors.centerIn: parent
                textFormat: Text.PlainText
                text: "✓"
                color: Commons.Color.popups.background
                font.family: Style.font.family
                font.pixelSize: Math.round(root.boxSize * 0.8)
                font.bold: true
              }

              Rectangle {
                visible: item.on && item.radio
                anchors.centerIn: parent
                width: Math.round(root.boxSize * 0.5)
                height: width
                radius: width / 2
                color: root.accent
              }
            }

            // Its icon from the icon theme, or its first letter without one.
            Item {
              id: icon
              anchors.left: box.right
              anchors.leftMargin: Style.spacing.controlGap
              anchors.verticalCenter: parent.verticalCenter
              width: item.hasIcon ? root.iconSize : 0
              height: root.iconSize

              Image {
                id: image
                anchors.fill: parent
                visible: status === Image.Ready
                // What the icon theme answers is a source as it is, or a path.
                source: {
                  var found = item.hasIcon ? Quickshell.iconPath(item.modelData.icon, true) : ""
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
                visible: item.hasIcon && image.status !== Image.Ready
                anchors.centerIn: parent
                textFormat: Text.PlainText
                text: item.isOption ? item.modelData.name.charAt(0) : ""
                color: root.foreground
                opacity: 0.7
                font.family: Style.font.family
                font.pixelSize: Math.round(root.iconSize * 0.7)
                font.bold: true
              }
            }

            Column {
              id: optionText
              anchors.left: icon.right
              anchors.leftMargin: item.hasIcon ? Style.spacing.controlGap : 0
              anchors.right: parent.right
              anchors.rightMargin: Style.spacing.controlPaddingX
              anchors.verticalCenter: parent.verticalCenter

              Text {
                width: parent.width
                textFormat: Text.PlainText
                text: item.isOption ? item.modelData.name : ""
                color: item.hot ? Style.hoverStateColor(root.foreground, root.accent) : root.foreground
                font.family: Style.font.family
                font.pixelSize: Style.font.body
                elide: Text.ElideRight
              }

              // A switch's help, and why a write of it failed.
              Text {
                visible: text !== ""
                width: parent.width
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: item.isOption ? item.modelData.help : ""
                color: root.foreground
                opacity: 0.5
                font.family: Style.font.family
                font.pixelSize: Style.font.bodySmall
              }

              Text {
                visible: text !== ""
                width: parent.width
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
                text: item.isOption && item.modelData.type === "bool" ? item.error : ""
                color: root.foreground
                opacity: 0.8
                font.family: Style.font.family
                font.pixelSize: Style.font.bodySmall
              }
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onPositionChanged: root.place(item.index, 0)
              onClicked: root.change(item.index, 0)
            }
          }
        }
      }
    }
  }
}
