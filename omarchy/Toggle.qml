import QtQuick

// The overlay entry the shell summons and hides (`super+p`, or a click on the
// space's name). The sidebar itself lives in the service, mapped for good, so
// this only forwards.
Item {
  property string omarchyPath: ""
  property var shell: null
  property var manifest: null
  property var service: null

  readonly property bool opened: !!service && service.sidebarOpen === true

  function open(payloadJson) {
    if (service) service.openSidebar()
  }

  function close() {
    if (service) service.closeSidebar()
  }
}
