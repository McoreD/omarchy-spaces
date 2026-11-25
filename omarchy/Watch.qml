import QtQuick
import Quickshell.Io
import "Spaces.mjs" as Spaces

// The monitors and the spaces, kept current by `space watch`, which prints all
// of it again, a line of JSON, whenever it changes: a window opening or
// closing, a switch, a terminal changing folder. The service runs one for the
// sidebar and the bar.
//
// It alone says which space and tab you are on. What shows that binds to
// `now`, and neither copies it nor asks Hyprland too: that answers in its
// own time, and a copy of an answer from before isn't corrected by the next
// line, which is only printed once something changes. Something shown ahead of
// it, like a row where you dropped it, is kept apart and laid over it, until
// it agrees or a moment has passed.
//
// `space watch` ends when Hyprland restarts or its binary is replaced, so it
// is started again, a little later each time it fails in a row. So is one that
// couldn't start, as when `space` isn't installed yet.
Item {
  id: root

  // The last line, as Spaces.parseState gives it.
  property var now: Spaces.EMPTY
  readonly property var rows: root.now.spaces
  // Whether a line has arrived since it started.
  property bool ready: false
  // Why it isn't running, when it keeps failing before printing anything.
  property string error: ""
  property int failures: 0
  // What it exited with. One that couldn't start never exits.
  property var exitCode: null

  function ended() {
    root.ready = false
    root.failures++
    if (root.exitCode === null) root.error = "The space command isn't installed: run install.sh in the plugin's folder"
    else if (root.failures > 1 && !root.error) root.error = "space watch exited with " + root.exitCode
    again.interval = Math.min(10000, 250 * Math.pow(2, Math.min(root.failures, 6)))
    again.restart()
  }

  Process {
    id: proc
    // With the sounds when an agent needs you: this is the watch you hear.
    command: ["space", "watch", "--sounds"]
    running: true
    stdout: SplitParser {
      onRead: function(line) {
        try {
          root.now = Spaces.parseState(line)
          root.ready = true
          root.error = ""
          root.failures = 0
        } catch (e) {
          root.error = String(e.message || e)
        }
      }
    }
    stderr: StdioCollector {
      onStreamFinished: if (!root.ready && text.trim()) root.error = text.trim()
    }
    onExited: function(exitCode) { root.exitCode = exitCode }
    // Later, once `exited` has said how.
    onRunningChanged: if (!proc.running) Qt.callLater(root.ended)
  }

  Timer {
    id: again
    onTriggered: {
      root.exitCode = null
      proc.running = true
    }
  }
}
