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
//
// While the sidebar shows on this screen, the tabs move over to where the
// windows begin, beside it, so they sit over the space's windows they belong
// to, and the name stays over the sidebar, where the spaces are. They move on
// the curve the windows slide aside on. Only a widget whose left edge stays put
// as it grows can do this, as one in the bar's left section does: anywhere
// else, growing would move it out from under the edge it lines up with.
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

  // Where this widget's left edge is along its bar, which spans its screen.
  property real origin: 0
  function place() {
    var content = root.QsWindow.contentItem
    if (content) root.origin = content.mapFromItem(root, 0, 0).x
  }
  // The widget moves when something before it on the bar changes size, which
  // moves one of the items it sits in.
  function follow() {
    for (var item = root; item; item = item.parent) item.xChanged.connect(root.place)
    root.place()
  }
  Component.onCompleted: root.follow()
  onParentChanged: root.follow()

  // Whether the tabs line up beside the sidebar: it is open on this bar's screen.
  readonly property bool beside: {
    if (root.vertical || root.fixedEdge !== "start" || !root.sidebar || !root.sidebar.opened) return false
    var window = root.QsWindow.window
    var screen = window ? window.screen : null
    return !!screen && !!root.sidebar.screen && screen.name === root.sidebar.screen.name
  }

  // Where the tabs start: after the name, or, beside the sidebar, where the
  // windows begin, so the first tab's hover lines up with their edge.
  readonly property real tabsAfterName: root.padding + head.implicitWidth + Style.spacing.xs
  readonly property real tabsBeside: root.sidebar
    ? root.sidebar.windowsEdge - root.origin - Style.spacing.xxs : root.tabsAfterName
  property real tabsX: root.beside ? Math.max(root.tabsAfterName, root.tabsBeside) : root.tabsAfterName
  Behavior on tabsX {
    enabled: !root.vertical
    NumberAnimation {
      duration: root.sidebar && !Style.reduceMotion ? root.sidebar.motion.duration : 0
      easing.type: Easing.BezierSpline
      easing.bezierCurve: root.sidebar ? root.sidebar.curve : [0, 0.75, 0.15, 1, 1, 1]
    }
  }
  onBesideChanged: root.place()

  function tabLabel(tab) {
    if (root.vertical) return String(tab.slot)
    return tab.slot + "  " + tab.label + (tab.windows > 1 ? "  +" + (tab.windows - 1) : "")
  }

  Tabs {
    id: tabs
    // One `space watch` for the bar and the sidebar: the service's.
    watch: root.service && root.service.watch ? root.service.watch : null
  }

  // The space's name; click it to show or hide the sidebar. It stays put as
  // the sidebar comes and goes, and the line between it and the tabs fades
  // while they sit apart.
  Row {
    id: head
    visible: !root.vertical
    x: root.padding
    height: root.barSize
    spacing: Style.spacing.xs

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
      opacity: root.beside ? 0 : 0.25
      Behavior on opacity { NumberAnimation { duration: Style.duration(160) } }
    }
  }

  Row {
    id: row
    visible: !root.vertical
    x: root.tabsX
    height: root.barSize
    spacing: Style.spacing.xs

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
