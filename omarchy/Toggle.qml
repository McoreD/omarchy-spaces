import QtQuick

// The overlay entry the shell summons and hides: the sidebar's search
// (`super+p`). The sidebar itself lives in the service, mapped for good, so
// this only forwards. The shell counts it open while you are searching.
Item {
  property string omarchyPath: ""
  property var shell: null
  property var manifest: null
  property var service: null

  readonly property bool opened: !!service && service.searching === true

  function open(payloadJson) {
    if (service) service.search()
  }

  function close() {
    if (service) service.endSearch()
  }

  // Show or hide the sidebar without taking the keyboard: `super+b`, and the
  // space's name on the bar. Called through `omarchy-shell shell call
  // com.t4t5.spaces showOrHide`.
  function showOrHide(arg) {
    if (service) service.showOrHide()
  }

  // Rename the space you are in, typing over its name in the sidebar:
  // `super+shift+r`. Called through `omarchy-shell shell call
  // com.t4t5.spaces rename`.
  function rename(arg) {
    if (service) service.rename()
  }

  // Name the tab you are on, typing on the bar: `super+,`. Called
  // through `omarchy-shell shell call com.t4t5.spaces renameTab`.
  function renameTab(arg) {
    if (service) service.renameTab()
  }

  // Pick a space to move the tab you are on to, and go there with it. Called
  // through `omarchy-shell shell call com.t4t5.spaces moveTab`.
  function moveTab(arg) {
    if (service) service.moveTab()
  }
}
