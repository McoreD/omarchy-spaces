import QtQuick

// The overlay entry the shell summons and hides (`super+p`). The sidebar
// itself lives in the service, mapped for good, so this only forwards.
//
// The shell counts it open while the sidebar holds the keyboard. A sidebar
// that shows but has handed the keyboard back counts as closed, so `super+p`
// summons it, which takes the keyboard back, and hides it the next time.
Item {
  property string omarchyPath: ""
  property var shell: null
  property var manifest: null
  property var service: null

  readonly property bool opened: !!service && service.sidebarFocused === true

  function open(payloadJson) {
    if (service) service.openSidebar()
  }

  function close() {
    if (service) service.closeSidebar()
  }

  // The space's name on the bar: show the sidebar, or hide it if it shows,
  // whether or not it holds the keyboard. Called through `omarchy-shell shell
  // call org.ren.renspace showOrHide`.
  function showOrHide(arg) {
    if (service) service.showOrHideSidebar()
  }
}
