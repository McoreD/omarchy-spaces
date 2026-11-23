pragma ComponentBehavior: Bound

import QtQuick
import qs.Commons
import qs.Commons as Commons

// The spaces list, drawn on the base by the left edge. The sidebar
// (Sidebar.qml) holds the rows, the filter and the cursor.
Item {
  id: root

  property var sidebar: null
  property color foreground: Commons.Color.bar.text

  readonly property color accent: Commons.Color.accent
  readonly property color selectedBackground: Commons.Color.menu.selectedBackground
  readonly property string fontFamily: Style.font.family
  readonly property int rowHeight: Math.round((Style.font.title + Style.font.bodySmall) * 1.35) + Style.spacing.controlPaddingY * 2
  readonly property int headerHeight: Math.max(Style.space(30), Style.font.subtitle + Style.spacing.controlPaddingY * 2)

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
      text: root.sidebar.filterText || "Spaces"
      color: root.foreground
      opacity: root.sidebar.filterText ? 1 : 0.5
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
        model: root.sidebar.shownRows
        clip: true
        spacing: Style.spacing.xxs
        boundsBehavior: Flickable.StopAtBounds

        delegate: Rectangle {
          id: rowItem
          required property var modelData
          required property int index
          readonly property bool selected: index === root.sidebar.selectedIndex

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

          MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onContainsMouseChanged: if (containsMouse) root.sidebar.selectedIndex = rowItem.index
            onClicked: root.sidebar.activate(rowItem.index)
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
  }
}
