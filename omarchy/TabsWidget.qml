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
// the keyboard (Sidebar.qml) and the tab shows what you type. So does Rename in the menu a right-click on a tab
// opens (SpaceMenu.qml). On a vertical bar there is room only for the tabs'
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
//
// With `"toggle": true` on its entry (Spaces.barToggle), the sidebar's
// header moves up onto the bar, in place of Omarchy's menu, which you take
// off it: the logo over the rail, which shows or hides the sidebar, and once
// it expands, its title and a toggle riding the windows' edge to the right.
// A right-click on the logo opens the menu.
BarWidget {
  id: root
  moduleName: "com.t4t5.spaces"

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
    return shell && typeof shell.serviceFor === "function" ? shell.serviceFor("com.t4t5.spaces") : null
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
    // Its number and what you last used on it: a new tab, with nothing open
    // on it yet, is just its number.
    return tab.slot + (tab.label ? "  " + tab.label : "")
  }

  // Opens a tab's menu at a point on the bar's window, which need not start
  // at its screen's top when the bar is at the bottom.
  function openTabMenu(slot, x, y) {
    var window = root.QsWindow.window
    if (!root.sidebar || !window || !window.screen) return
    var top = root.bar && root.bar.position === "bottom" ? window.screen.height - window.height : 0
    root.sidebar.openTabMenu(slot, x, top + y, window.screen)
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

  // The sidebar's header, laid out as the list lays it out under the bar
  // (SidebarList.qml), on the screen's coordinates: Omarchy's logo in the
  // rail's icon column, and once the sidebar expands, its title in line with
  // the spaces' names and the toggle where a row's highlight ends. The toggle
  // rides the cut, coming out from behind the logo as the sidebar slides
  // open, and the title is uncovered in the room it leaves.
  Loader {
    active: root.parts && root.sidebar.headerOnBar
    x: -root.origin.x
    y: -root.origin.y
    width: root.edge
    height: root.barSize

    sourceComponent: Item {
      id: header
      readonly property var sidebar: root.sidebar
      readonly property int inset: header.sidebar.inset
      readonly property int cell: header.sidebar.cell
      // How much of the sidebar shows on this screen.
      readonly property real shown: {
        var here = !!header.sidebar.screen && header.sidebar.screen.name === root.screenName
        return here ? header.sidebar.revealed : header.sidebar.railWidth
      }
      readonly property real highlightWidth: Math.max(header.cell, header.shown - header.inset * 2)
      // How much the title and the toggle show, as the list's text
      // (SidebarList.qml): only while the sidebar is expanded on this screen,
      // fading quickly. The logo stays, to expand it again.
      property real textShown: header.sidebar.opened && !!header.sidebar.screen
        && header.sidebar.screen.name === root.screenName ? 1 : 0
      Behavior on textShown { NumberAnimation { duration: Style.duration(120) } }
      // Where the toggle is, and where the title starts, past the logo.
      readonly property real toggleX: header.inset + header.highlightWidth - header.cell
      readonly property real afterLogo: header.inset + header.cell
      readonly property real titleX: header.afterLogo + header.sidebar.textGap

      function showOrHide() {
        var window = root.QsWindow.window
        header.sidebar.showOrHide(window ? window.screen : null)
      }

      // The logo, as the menu draws it, for the sidebar; a right-click
      // opens the menu.
      HeaderButton {
        x: header.inset
        width: header.cell
        text: "\ue900"
        fontFamily: "omarchy"
        fontSize: Style.bar.iconFont
        restingOpacity: 1
        onClicked: function(button) {
          if (button === Qt.RightButton) root.run(["omarchy-shell", "shell", "toggle", "omarchy.menu", "{\"menu\":\"root\"}"])
          else header.showOrHide()
        }
      }

      Item {
        x: header.titleX
        width: Math.max(0, header.toggleX - header.titleX)
        height: parent.height
        clip: true

        // As wide as its room once expanded, so while the sidebar slides
        // it is uncovered rather than cut short.
        Text {
          width: header.sidebar.panelWidth - header.inset - header.cell - header.titleX
          height: parent.height
          verticalAlignment: Text.AlignVCenter
          textFormat: Text.PlainText
          text: "Spaces"
          color: root.foreground
          opacity: 0.5 * header.textShown
          font.family: Style.font.family
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }
      }

      // Past the logo, so the toggle shows only once it has come out from
      // behind it.
      Item {
        opacity: header.textShown
        x: header.afterLogo
        width: Math.max(0, header.toggleX + header.cell - header.afterLogo)
        height: parent.height
        clip: true

        HeaderButton {
          x: header.toggleX - header.afterLogo
          width: header.cell
          enabled: x >= 0
          // The sidebar, docked on the left (nf-cod-layout_sidebar_left).
          text: "\u{ebf3}"
          onClicked: header.showOrHide()
        }
      }
    }
  }

  // A button in the sidebar's header on the bar, drawn as the list draws its
  // controls: an icon centred by the ink it draws, lit under the pointer, on
  // a control's box, unless the bar is too thin for one: then as far in from
  // the bar's edges as a tab's.
  component HeaderButton: Item {
    id: button
    property alias text: icon.text
    property string fontFamily: Style.font.family
    property int fontSize: Style.font.icon
    property real restingOpacity: 0.6
    readonly property bool lit: mouse.containsMouse
    signal clicked(int button)

    height: root.barSize

    Rectangle {
      readonly property real size: Math.min(root.sidebar ? root.sidebar.controlSize : 0, root.barSize - Style.spacing.xs * 2)
      anchors.centerIn: parent
      width: size
      height: size
      radius: root.sidebar ? root.sidebar.badgeRadius : 0
      color: button.lit ? Qt.rgba(root.hover.r, root.hover.g, root.hover.b, root.hover.a * 0.6) : "transparent"
    }

    Text {
      id: icon
      x: Math.round((button.width - ink.tightBoundingRect.width) / 2 - ink.tightBoundingRect.x)
      y: Math.round((button.height - ink.tightBoundingRect.height) / 2 - (icon.baselineOffset + ink.tightBoundingRect.y))
      textFormat: Text.PlainText
      color: button.lit ? root.accent : root.foreground
      opacity: button.lit ? 1 : button.restingOpacity
      font.family: button.fontFamily
      font.pixelSize: button.fontSize
    }

    TextMetrics {
      id: ink
      font: icon.font
      text: icon.text
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      acceptedButtons: Qt.LeftButton | Qt.RightButton
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: function(event) { button.clicked(event.button) }
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
        hasMenu: true
        menuOpen: !!root.sidebar && root.sidebar.menuTabFor === root.model.space + ":" + modelData.slot
          && !!root.sidebar.menuScreen && root.sidebar.menuScreen.name === root.screenName
        onDoubleClicked: if (root.sidebar) root.sidebar.renameTab(modelData.slot)
        onMenuRequested: function(x, y) { root.openTabMenu(modelData.slot, x, y) }
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
    // Whether a right-click opens its menu: not the `+`'s, nor on a vertical
    // bar, which has no room to type a name. Where on the bar's window it was
    // right-clicked, and whether its menu is open.
    property bool hasMenu: false
    signal menuRequested(real x, real y)
    property bool menuOpen: false

    // Room after the label for the status's dot, on a horizontal bar.
    readonly property real dotRoom: button.status !== "" && !root.vertical ? dot.width + Style.spacing.sm : 0

    // A share of the room before the bar's centre caps a tab, not the `+`.
    property real cap: Infinity
    readonly property real natural: Math.min((button.editing ? editor.implicitWidth : label.implicitWidth) + button.dotRoom + Style.spacing.rowPaddingX * 2, Style.space(220))
    // Uncapped while you type a name, so you see all of it.
    width: root.vertical ? root.barSize : (button.editing ? button.natural : Math.min(button.natural, button.cap))
    height: root.vertical ? Style.space(26) : root.barSize

    // Outlined like a text field while you type its name, so it reads as one.
    Rectangle {
      anchors.fill: parent
      anchors.margins: Style.spacing.xs
      radius: Style.cornerRadius
      color: button.editing ? Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.08)
        : button.menuOpen || (mouse.containsMouse && !button.current) ? root.hover : "transparent"
      border.width: button.editing ? Math.max(1, Style.space(1)) : 0
      border.color: Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.6)
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
        // Wide enough for the placeholder while there's no name.
        width: text === "" ? placeholder.x + placeholder.implicitWidth : Math.min(implicitWidth, room)
        textFormat: Text.PlainText
        text: button.editing && root.sidebar ? root.sidebar.nameText : ""
        elide: Text.ElideLeft
        color: label.color
        font: label.font
        // A selection of nothing shows no highlight, so the caret shows instead.
        readonly property bool selected: button.editing && !!root.sidebar && root.sidebar.nameSelected && text !== ""

        // What to type, while there's no name: a tab with none (one of home's)
        // would otherwise show only its number.
        Text {
          id: placeholder
          visible: editName.text === ""
          x: caret.x + caret.width
          anchors.verticalCenter: parent.verticalCenter
          textFormat: Text.PlainText
          text: "name"
          color: root.foreground
          opacity: 0.35
          font.family: root.fontFamily
          font.pixelSize: root.fontSize
        }

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
      x: root.vertical ? parent.width * 0.7
        : button.editing ? editor.x + editor.implicitWidth + Style.spacing.sm
        : label.x + Math.min(label.contentWidth, label.width) + Style.spacing.sm
      y: root.vertical ? Style.spacing.xs : (parent.height - height) / 2
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: button.editing ? Qt.IBeamCursor : Qt.PointingHandCursor
      acceptedButtons: button.hasMenu ? Qt.LeftButton | Qt.RightButton : Qt.LeftButton
      onClicked: function(event) {
        if (button.editing) return
        if (event.button !== Qt.RightButton) {
          button.clicked()
          return
        }
        var at = mouse.mapToItem(null, event.x, event.y)
        button.menuRequested(at.x, at.y)
      }
      onDoubleClicked: function(event) {
        if (!button.editing && event.button === Qt.LeftButton) button.doubleClicked()
      }
    }
  }
}
