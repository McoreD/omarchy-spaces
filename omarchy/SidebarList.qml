pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons
import qs.Commons as Commons
import "Spaces.mjs" as Spaces

// The spaces list, drawn on the base by the left edge, with its search field
// at the bottom. The sidebar (Sidebar.qml) holds the rows, the search, the
// cursor and the rename; the keyboard reaches it through the sidebar's panel,
// so the field and a name being renamed draw their own caret. A right-click
// opens a space's menu (SpaceMenu.qml) where you clicked.
Item {
  id: root

  property var sidebar: null
  property color foreground: Commons.Color.bar.text

  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int rowHeight: Math.round((Style.font.title + Style.font.bodySmall) * 1.35) + Style.spacing.controlPaddingY * 2
  readonly property int headerHeight: Math.max(Style.space(30), Style.font.subtitle + Style.spacing.controlPaddingY * 2)
  readonly property int searchHeight: Style.font.body + Style.spacing.controlPaddingY * 4

  function reveal() {
    if (root.sidebar.shownRows.length > 0) list.positionViewAtIndex(root.sidebar.selectedIndex, ListView.Contain)
  }

  Connections {
    target: root.sidebar
    function onSelectedIndexChanged() { root.reveal() }
    function onShownRowsChanged() { Qt.callLater(root.reveal) }
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
      text: "Spaces"
      color: root.foreground
      opacity: 0.5
      font.family: root.fontFamily
      font.pixelSize: Style.font.subtitle
      elide: Text.ElideRight
    }

    Item {
      width: parent.width
      height: parent.height - root.headerHeight - root.searchHeight - parent.spacing * 2

      ListView {
        id: list
        anchors.fill: parent
        model: root.sidebar.shownRows
        clip: true
        spacing: Style.spacing.xxs
        boundsBehavior: Flickable.StopAtBounds

        delegate: Rectangle {
          id: rowItem
          required property var modelData
          required property int index
          readonly property bool selected: index === root.sidebar.selectedIndex
          readonly property bool renaming: root.sidebar.renaming !== "" && root.sidebar.renaming === modelData.name
          readonly property bool menuOpen: root.sidebar.menuFor !== "" && root.sidebar.menuFor === modelData.name

          width: ListView.view.width
          height: root.rowHeight
          radius: Style.cornerRadius
          // The cursor shows while searching; otherwise only under the
          // pointer, or while the row is being renamed or has its menu open.
          color: rowItem.renaming || rowItem.menuOpen
              || (!root.sidebar.menuRow && rowItem.selected && (root.sidebar.searching || rowMouse.containsMouse))
            ? root.selectedBackground : "transparent"
          border.width: rowItem.renaming ? 1 : 0
          border.color: root.accent

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
              id: nameText
              width: parent.width
              textFormat: Text.PlainText
              text: rowItem.renaming ? root.sidebar.renameText
                : rowItem.modelData.create ? "New space “" + rowItem.modelData.name + "”"
                : rowItem.modelData.name
              color: rowItem.selected || rowItem.modelData.create ? root.accent : root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.title
              font.bold: rowItem.modelData.active
              elide: rowItem.renaming ? Text.ElideLeft : Text.ElideRight

              // The keyboard reaches the rename through the sidebar's
              // panel, so the name draws its own caret.
              Rectangle {
                visible: rowItem.renaming
                x: Math.min(nameText.contentWidth, nameText.width) + 1
                width: Math.max(1, Style.space(2))
                height: Style.font.title + Style.spacing.xxs
                anchors.verticalCenter: parent.verticalCenter
                color: root.accent

                SequentialAnimation on opacity {
                  running: rowItem.renaming
                  loops: Animation.Infinite
                  alwaysRunToEnd: false
                  NumberAnimation { to: 1; duration: 0 }
                  PauseAnimation { duration: 530 }
                  NumberAnimation { to: 0; duration: 0 }
                  PauseAnimation { duration: 530 }
                }
              }
            }

            Text {
              width: parent.width
              textFormat: Text.PlainText
              text: root.sidebar.subtitle(rowItem.modelData)
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

          // Click to go there, right-click for the space's menu.
          MouseArea {
            id: rowMouse
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            cursorShape: rowItem.renaming ? Qt.IBeamCursor : Qt.PointingHandCursor
            onContainsMouseChanged: if (containsMouse) root.sidebar.select(rowItem.index)
            onClicked: function(mouse) {
              if (rowItem.renaming) return
              if (mouse.button !== Qt.RightButton) {
                root.sidebar.activate(rowItem.index)
              } else if (Spaces.rowActions(rowItem.modelData).length > 0) {
                // The base covers the screen, so its coordinates are the
                // screen's, which the menu's surface shares.
                var at = rowMouse.mapToItem(null, mouse.x, mouse.y)
                root.sidebar.openMenu(rowItem.modelData.name, at.x, at.y)
              }
            }
          }
        }
      }

      Text {
        anchors.centerIn: parent
        width: parent.width
        visible: root.sidebar.shownRows.length === 0 && (root.sidebar.error !== "" || root.sidebar.filterText !== "")
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
        text: root.sidebar.error !== "" ? root.sidebar.error : "No space matches “" + root.sidebar.filterText + "”"
        color: root.foreground
        opacity: 0.7
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }
    }

    // Click it, or press super+p, to search.
    Rectangle {
      id: searchField
      width: parent.width
      height: root.searchHeight
      radius: Style.cornerRadius
      color: root.sidebar.searching ? root.selectedBackground : "transparent"
      border.width: 1
      border.color: root.sidebar.searching ? root.accent : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.2)

      Row {
        anchors.left: parent.left
        anchors.right: hint.left
        anchors.leftMargin: Style.spacing.rowPaddingX
        anchors.rightMargin: Style.spacing.xs
        anchors.verticalCenter: parent.verticalCenter
        spacing: 0
        clip: true

        Text {
          id: query
          visible: root.sidebar.filterText !== ""
          width: Math.min(implicitWidth, parent.width - caret.width)
          textFormat: Text.PlainText
          text: root.sidebar.filterText
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideLeft
        }

        Rectangle {
          id: caret
          visible: root.sidebar.searching
          width: Math.max(1, Style.space(2))
          height: Style.font.body + Style.spacing.xxs
          anchors.verticalCenter: parent.verticalCenter
          color: root.accent

          SequentialAnimation on opacity {
            running: caret.visible
            loops: Animation.Infinite
            alwaysRunToEnd: false
            NumberAnimation { to: 1; duration: 0 }
            PauseAnimation { duration: 530 }
            NumberAnimation { to: 0; duration: 0 }
            PauseAnimation { duration: 530 }
          }
        }

        Text {
          visible: root.sidebar.filterText === ""
          leftPadding: root.sidebar.searching ? Style.spacing.xxs : 0
          textFormat: Text.PlainText
          text: "Find a space"
          color: root.foreground
          opacity: 0.4
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
        }
      }

      Text {
        id: hint
        anchors.right: parent.right
        anchors.rightMargin: Style.spacing.rowPaddingX
        anchors.verticalCenter: parent.verticalCenter
        textFormat: Text.PlainText
        text: root.sidebar.searching ? "esc" : "super+p"
        color: root.foreground
        opacity: 0.35
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
      }

      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.IBeamCursor
        onClicked: root.sidebar.search()
      }
    }
  }
}
