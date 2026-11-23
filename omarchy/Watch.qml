import QtQuick
import Quickshell.Io
import "Spaces.mjs" as Spaces

// The spaces, kept current by `space watch`, which prints them again, a line
// of JSON, whenever they change: a window opening or closing, a switch, a
// terminal changing folder. The service runs one for the sidebar and the bar.
//
// `space watch` ends when Hyprland restarts or its binary is replaced, so it
// is started again, a little later each time it fails in a row.
Item {
  id: root

  // Rows as Spaces.parseRows gives them.
  property var rows: []
  // Whether a list has arrived since it started.
  property bool ready: false
  // Why it isn't running, when it keeps failing before printing anything.
  property string error: ""
  property int failures: 0

  Process {
    id: proc
    command: ["space", "watch"]
    running: true
    stdout: SplitParser {
      onRead: function(line) {
        try {
          root.rows = Spaces.parseRows(line)
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
    onExited: function(exitCode) {
      root.ready = false
      root.failures++
      if (root.failures > 1 && !root.error) root.error = "space watch exited with " + exitCode
      again.interval = Math.min(10000, 250 * Math.pow(2, Math.min(root.failures, 6)))
      again.restart()
    }
  }

  Timer {
    id: again
    onTriggered: proc.running = true
  }
}
