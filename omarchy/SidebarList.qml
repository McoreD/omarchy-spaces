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
//
// It is laid out at the sidebar's whole width, and the base cuts it off where
// the windows begin (`shownWidth`). Each row starts with its icon in the
// rail's column, the part that always shows: the toggle, a badge for each
// space, the `+` and the search. What highlights a row ends just short of
// the cut, so collapsed it is a square around the icon, and it grows as the
// windows slide aside.
Item {
  id: root

  property var sidebar: null
  property color foreground: Commons.Color.bar.text
  // How much of it shows, from its left edge.
  property real shownWidth: width
  // The screen it is drawn on.
  property var screen: null

  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int margin: Style.spacing.md
  // The icon column at the start of each row, the rail less its margins.
  readonly property int slot: root.sidebar.railWidth - root.margin * 2
  readonly property int badgeSize: Math.min(root.slot, Style.space(24))
  // How wide a row's highlight is: to just short of the cut.
  readonly property real highlightWidth: Math.max(root.slot, root.shownWidth - root.margin * 2)
  readonly property int rowHeight: Math.round((Style.font.title + Style.font.bodySmall) * 1.35) + Style.spacing.controlPaddingY * 2
  readonly property int headerHeight: Math.max(Style.space(30), Style.font.subtitle + Style.spacing.controlPaddingY * 2)
  readonly property int searchHeight: Style.font.body + Style.spacing.controlPaddingY * 4
  // Where a row's text starts, past its icon.
  readonly property int textX: root.slot + Style.spacing.controlGap

  function reveal() {
    if (root.sidebar.shownRows.length > 0) list.positionViewAtIndex(root.sidebar.selectedIndex, ListView.Contain)
  }

  Connections {
    target: root.sidebar
    function onSelectedIndexChanged() { root.reveal() }
    function onShownRowsChanged() { Qt.callLater(root.reveal) }
  }

  // An icon centred in the icon column.
  component SlotIcon: Text {
    property bool lit: false
    width: root.slot
    height: parent.height
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    textFormat: Text.PlainText
    color: lit ? root.accent : root.foreground
    opacity: lit ? 1 : 0.6
    font.family: root.fontFamily
    font.pixelSize: Style.font.icon
  }

  // A row's highlight, ending just short of the cut.
  component Highlight: Rectangle {
    property bool lit: false
    width: Math.min(parent.width, root.highlightWidth)
    height: parent.height
    radius: Style.cornerRadius
    color: lit ? root.selectedBackground : "transparent"
  }

  Column {
    anchors.fill: parent
    anchors.margins: root.margin
    spacing: Style.spacing.sm

    // The toggle, then the title.
    Item {
      width: parent.width
      height: root.headerHeight

      Highlight {
        width: root.slot
        lit: toggleMouse.containsMouse
      }

      SlotIcon {
        // The sidebar, docked on the left (nf-cod-layout_sidebar_left).
        text: "\u{ebf3}"
        lit: toggleMouse.containsMouse
      }

      Text {
        x: root.textX
        width: parent.width - x
        height: parent.height
        verticalAlignment: Text.AlignVCenter
        textFormat: Text.PlainText
        text: "Spaces"
        color: root.foreground
        opacity: 0.5
        font.family: root.fontFamily
        font.pixelSize: Style.font.subtitle
        elide: Text.ElideRight
      }

      MouseArea {
        id: toggleMouse
        width: root.slot
        height: parent.height
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.sidebar.showOrHide(root.screen)
      }
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

        delegate: Item {
          id: rowItem
          required property var modelData
          required property int index
          readonly property bool selected: index === root.sidebar.selectedIndex
          // Its name is being typed.
          readonly property bool renaming: root.sidebar.renaming !== "" && root.sidebar.renaming === modelData.id
          readonly property bool menuOpen: root.sidebar.menuFor !== "" && root.sidebar.menuFor === modelData.id
          readonly property bool home: modelData.id === "home"
          readonly property color hue: rowItem.home ? root.foreground
            : (Spaces.badgeColor(rowItem.modelData.label, root.sidebar.palette) || root.foreground)

          width: ListView.view.width
          height: root.rowHeight

          // The cursor shows while searching; otherwise only under the
          // pointer, or while the row is being renamed or has its menu open.
          Highlight {
            lit: rowItem.renaming || rowItem.menuOpen
              || (!root.sidebar.menuRow && rowItem.selected && (root.sidebar.searching || rowMouse.containsMouse))
            border.width: rowItem.renaming ? 1 : 0
            border.color: root.accent
          }

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

          // Its badge: two letters on a tint of its colour, or a house for
          // home, which is no folder's.
          Rectangle {
            x: (root.slot - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            width: root.badgeSize
            height: root.badgeSize
            radius: Math.round(root.badgeSize / 4)
            color: Qt.rgba(rowItem.hue.r, rowItem.hue.g, rowItem.hue.b, rowItem.home ? 0.1 : 0.16)

            Text {
              anchors.fill: parent
              horizontalAlignment: Text.AlignHCenter
              verticalAlignment: Text.AlignVCenter
              textFormat: Text.PlainText
              // A house (nf-md-home_outline).
              text: rowItem.home ? "\u{f06a1}" : Spaces.monogram(rowItem.modelData.label)
              color: rowItem.hue
              opacity: rowItem.home ? 0.8 : 1
              font.family: root.fontFamily
              font.pixelSize: rowItem.home ? Style.font.icon : Math.round(root.badgeSize * 0.42)
              font.bold: true
            }
          }

          Column {
            anchors.left: parent.left
            anchors.right: windowCount.left
            anchors.leftMargin: root.textX
            anchors.rightMargin: Style.spacing.controlGap
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.spacing.xxs

            Text {
              id: nameText
              width: parent.width
              textFormat: Text.PlainText
              text: rowItem.renaming ? root.sidebar.nameText : rowItem.modelData.label
              color: rowItem.selected ? root.accent : root.foreground
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
                root.sidebar.openMenu(rowItem.modelData.id, at.x, at.y, root.screen)
              }
            }
          }
        }

        // The `+`: a new space with a terminal in the folder you are in.
        footer: Item {
          width: ListView.view ? ListView.view.width : 0
          height: root.rowHeight * 0.75 + Style.spacing.xxs

          Item {
            y: Style.spacing.xxs
            width: parent.width
            height: parent.height - y

            Highlight {
              lit: plusMouse.containsMouse
            }

            SlotIcon {
              // A plus (nf-md-plus).
              text: "\u{f0415}"
              lit: plusMouse.containsMouse
            }

            Text {
              x: root.textX
              width: parent.width - x
              height: parent.height
              verticalAlignment: Text.AlignVCenter
              textFormat: Text.PlainText
              text: "New space"
              color: plusMouse.containsMouse ? root.accent : root.foreground
              opacity: plusMouse.containsMouse ? 1 : 0.6
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              elide: Text.ElideRight
            }

            MouseArea {
              id: plusMouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.sidebar.newSpace()
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

    // Click it, or press super+p, to search. Its box ends just short of the
    // cut, as a row's highlight does; what it holds is laid out at its whole
    // width, and cut off with it.
    Item {
      width: parent.width
      height: root.searchHeight

      Rectangle {
        id: searchField
        width: Math.min(parent.width, root.highlightWidth)
        height: parent.height
        radius: Style.cornerRadius
        clip: true
        color: root.sidebar.searching ? root.selectedBackground : "transparent"
        border.width: 1
        border.color: root.sidebar.searching ? root.accent : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.2)

        Item {
          width: searchField.parent.width
          height: parent.height

          SlotIcon {
            // A magnifying glass (nf-md-magnify).
            text: "\u{f0349}"
            lit: root.sidebar.searching
          }

          Row {
            anchors.left: parent.left
            anchors.leftMargin: root.textX
            anchors.right: hint.left
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
        }

        MouseArea {
          anchors.fill: parent
          cursorShape: Qt.IBeamCursor
          onClicked: root.sidebar.search()
        }
      }
    }
  }
}
