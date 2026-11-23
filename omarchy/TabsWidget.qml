pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes
import Quickshell
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Spaces.mjs" as Spaces

// The active space's tabs on Omarchy's bar, in place of its workspaces: a tab
// per workspace. Clicks on tabs run `space ws`, which stays the only thing
// that changes workspaces. Double-clicking a tab names it: the sidebar holds
// the keyboard (Sidebar.qml) and the tab shows what you type. On a vertical bar there is room only for the tabs'
// numbers.
//
// The bar parts where the windows begin, by the sidebar's rail or by the
// whole sidebar. Before that line it shows the background, shaded as the
// sidebar below it is, with the menu on it. Past it, the tabs and the rest of
// the bar are the space's, over its windows, from a rounded corner. Which
// space you are in shows in the sidebar's column, so the bar doesn't name it.
// The line moves with the windows as they slide aside (Sidebar.qml's `edge`),
// carrying the tabs, so they move as far and as fast as the windows do. Only
// a widget whose left edge stays put as it grows can do this, as one in the
// bar's left section does: anywhere else, growing would move it out from
// under the line.
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
  // The space's corner rounds as Hyprland rounds windows.
  readonly property real cornerRadius: Style.cornerRadius

  implicitWidth: root.vertical ? root.barSize : row.x + row.implicitWidth + root.padding
  implicitHeight: root.vertical ? column.implicitHeight : root.barSize

  function run(args) {
    Quickshell.execDetached(args)
  }

  // The service, which runs the one `space watch` and holds the sidebar.
  readonly property var service: {
    var shell = root.bar ? root.bar.shell : null
    if (shell && shell.services !== undefined) shell.services
    return shell && typeof shell.serviceFor === "function" ? shell.serviceFor("org.ren.renspace") : null
  }
  readonly property var sidebar: root.service && root.service.sidebar ? root.service.sidebar : null

  // Where this widget sits on its bar, which spans its screen.
  property point origin: Qt.point(0, 0)
  function place() {
    var content = root.QsWindow.contentItem
    if (content) root.origin = content.mapFromItem(root, 0, 0)
  }
  // The widget moves when something before it on the bar changes size, which
  // moves one of the items it sits in. Read once the bar has laid itself out:
  // resizing, the widget's slot centres it in room that catches up a moment
  // later, which would otherwise move it for that moment.
  function placeLater() {
    Qt.callLater(root.place)
  }
  function follow() {
    for (var item = root; item; item = item.parent) item.xChanged.connect(root.placeLater)
    root.placeLater()
  }
  Component.onCompleted: root.follow()
  onParentChanged: root.follow()

  readonly property string screenName: {
    var window = root.QsWindow.window
    return window && window.screen ? window.screen.name : ""
  }
  // Whether the sidebar parts this bar: the widget can follow its edge.
  readonly property bool parts: !root.vertical && root.fixedEdge === "start" && !!root.sidebar
  // Where the windows begin on this screen, as they slide.
  readonly property real edge: {
    if (!root.parts) return 0
    var here = !!root.sidebar.screen && root.sidebar.screen.name === root.screenName
    return here ? root.sidebar.edge : root.sidebar.railEdge
  }
  onPartsChanged: root.placeLater()

  // Where the tabs start: at the space's part of the bar, so a tab's hover
  // background stands as far from the space's side as from its top.
  readonly property real tabsX: Math.max(root.padding, root.edge - root.origin.x)

  // Where the bar's centre starts, its clock, as the bar window keeps it. Past
  // the sidebar the tabs have less room before it, so when they would reach
  // it the longest share what the shorter leave, their names cut short, their
  // numbers kept. The
  // widgets that hang off the clock's start, its indicators, are other
  // plugins' and can't be measured, so room for two of their icons is kept
  // when the bar has any.
  readonly property real centerStart: {
    var window = root.QsWindow.window
    return window && typeof window.peekEdge === "number" && window.peekEdge > 0 ? window.peekEdge : Infinity
  }
  readonly property real beforeCenter: {
    var layout = root.bar ? root.bar.layoutConfig : null
    var hanging = layout && Array.isArray(layout.centerBegin) && layout.centerBegin.length > 0
    return Style.spacing.lg + (hanging ? Style.bar.iconSlot * 2 : 0)
  }
  readonly property real tabCap: {
    var count = root.model.tabs.length
    if (count === 0 || root.centerStart === Infinity) return Infinity
    var room = root.centerStart - root.beforeCenter - root.origin.x - root.tabsX
      - newTab.width - row.spacing * count
    var widths = []
    for (var i = 0; i < tabRepeater.count; i++) {
      var tab = tabRepeater.itemAt(i)
      if (tab) widths.push(tab.natural)
    }
    return Math.max(Style.spacing.rowPaddingX * 2 + root.fontSize * 2, Spaces.fairCap(widths, room))
  }

  function tabLabel(tab) {
    if (root.vertical) return String(tab.slot)
    return tab.slot + "  " + tab.label + (tab.windows > 1 ? "  +" + (tab.windows - 1) : "")
  }

  Tabs {
    id: tabs
    rows: root.service && root.service.watch ? root.service.watch.rows : []
  }

  // The background, from the screen's edge to where the windows begin, over
  // the bar's whole thickness, and the space's part of the bar's rounded
  // corner past it. Drawn under the tabs, and over the menu before them,
  // which it shades along with the rest.
  Item {
    id: recess
    visible: !root.vertical && root.edge > 0.5
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize
    readonly property color shade: {
      var bar = Commons.Color.bar.background
      return Qt.rgba(0, 0, 0, Spaces.recessAlpha(bar.r, bar.g, bar.b))
    }

    Rectangle {
      anchors.fill: parent
      color: recess.shade
    }

    // The square at the corner, less the quarter circle the corner rounds by.
    Shape {
      visible: root.cornerRadius > 0
      x: recess.width
      width: root.cornerRadius
      height: root.cornerRadius
      preferredRendererType: Shape.CurveRenderer

      ShapePath {
        strokeWidth: -1
        fillColor: recess.shade
        startX: 0; startY: 0
        PathLine { x: root.cornerRadius; y: 0 }
        PathArc {
          x: 0; y: root.cornerRadius
          radiusX: root.cornerRadius; radiusY: root.cornerRadius
          direction: PathArc.Counterclockwise
        }
        PathLine { x: 0; y: 0 }
      }
    }
  }

  Row {
    id: row
    visible: !root.vertical
    x: root.tabsX
    height: root.barSize
    spacing: Style.spacing.xs

    Repeater {
      id: tabRepeater
      model: root.vertical ? [] : root.model.tabs

      TabButton {
        required property var modelData
        text: root.tabLabel(modelData)
        number: String(modelData.slot)
        editing: !!root.sidebar && root.sidebar.renaming === root.model.space + ":" + modelData.slot
        status: modelData.status
        current: modelData.active
        cap: root.tabCap
        onClicked: root.run(["space", "ws", String(modelData.slot)])
        onDoubleClicked: if (root.sidebar) root.sidebar.renameTab(modelData.slot)
      }
    }

    TabButton {
      id: newTab
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
        status: modelData.status
        current: modelData.active
        onClicked: root.run(["space", "ws", String(modelData.slot)])
      }
    }
  }

  component TabButton: Item {
    id: button
    property string text: ""
    property bool current: false
    // What most needs you among the agents on the tab, if any.
    property string status: ""
    // Its number, and whether you are typing a name for it.
    property string number: ""
    property bool editing: false
    signal clicked()
    signal doubleClicked()

    // Room after the label for the status's dot, on a horizontal bar.
    readonly property real dotRoom: button.status !== "" && !root.vertical ? dot.width + Style.spacing.sm : 0

    // A share of the room before the bar's centre caps a tab, not the `+`.
    property real cap: Infinity
    readonly property real natural: Math.min((button.editing ? editor.implicitWidth : label.implicitWidth) + button.dotRoom + Style.spacing.rowPaddingX * 2, Style.space(220))
    // Uncapped while you type a name, so you see all of it.
    width: root.vertical ? root.barSize : (button.editing ? button.natural : Math.min(button.natural, button.cap))
    height: root.vertical ? Style.space(26) : root.barSize

    Rectangle {
      anchors.fill: parent
      anchors.margins: Style.spacing.xs
      radius: Style.cornerRadius
      color: mouse.containsMouse && !button.current ? root.hover : "transparent"
    }

    Text {
      id: label
      visible: !button.editing
      anchors.fill: parent
      anchors.leftMargin: root.vertical ? 0 : Style.spacing.rowPaddingX
      anchors.rightMargin: root.vertical ? 0 : Style.spacing.rowPaddingX + button.dotRoom
      verticalAlignment: Text.AlignVCenter
      horizontalAlignment: Text.AlignHCenter
      textFormat: Text.PlainText
      text: button.text
      elide: Text.ElideRight
      color: button.current ? root.accent : root.foreground
      opacity: button.current ? 1 : 0.6
      font.family: root.fontFamily
      font.pixelSize: root.fontSize
      font.bold: button.current
    }

    // A name being typed, after the tab's number, with its own selection and
    // caret, as the sidebar's rows draw theirs: the keyboard is the sidebar's.
    Row {
      id: editor
      visible: button.editing && !root.vertical
      x: Style.spacing.rowPaddingX
      anchors.verticalCenter: parent.verticalCenter

      Text {
        id: editNumber
        textFormat: Text.PlainText
        text: button.number + "  "
        color: label.color
        font: label.font
      }

      Text {
        id: editName
        readonly property real room: Style.space(220) - Style.spacing.rowPaddingX * 2 - editNumber.implicitWidth - caret.width - 1
        width: Math.min(implicitWidth, room)
        textFormat: Text.PlainText
        text: button.editing && root.sidebar ? root.sidebar.nameText : ""
        elide: Text.ElideLeft
        color: label.color
        font: label.font
        readonly property bool selected: button.editing && !!root.sidebar && root.sidebar.nameSelected

        Rectangle {
          visible: editName.selected
          z: -1
          width: editName.contentWidth
          height: root.fontSize + Style.spacing.xxs
          anchors.verticalCenter: parent.verticalCenter
          radius: Math.max(1, Style.space(2))
          color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.3)
        }

        Rectangle {
          id: caret
          visible: button.editing && !editName.selected
          x: editName.contentWidth + 1
          width: Math.max(1, Style.space(2))
          height: root.fontSize + Style.spacing.xxs
          anchors.verticalCenter: parent.verticalCenter
          color: root.accent

          SequentialAnimation on opacity {
            running: button.editing
            loops: Animation.Infinite
            alwaysRunToEnd: false
            NumberAnimation { to: 1; duration: 0 }
            PauseAnimation { duration: 530 }
            NumberAnimation { to: 0; duration: 0 }
            PauseAnimation { duration: 530 }
          }
        }
      }
    }

    // After the label on a horizontal bar; on a vertical one, on the
    // number's corner.
    StatusDot {
      id: dot
      status: button.status
      colors: root.sidebar ? root.sidebar.statusColors : null
      workingColor: root.foreground
      size: Math.max(5, Math.round(root.fontSize * 0.45))
      x: root.vertical ? parent.width * 0.7 : label.x + Math.min(label.contentWidth, label.width) + Style.spacing.sm
      y: root.vertical ? Style.spacing.xs : (parent.height - height) / 2
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: button.editing ? Qt.IBeamCursor : Qt.PointingHandCursor
      onClicked: if (!button.editing) button.clicked()
      onDoubleClicked: if (!button.editing) button.doubleClicked()
    }
  }
}
