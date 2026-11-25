import QtQuick
import Quickshell
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
// couldn't start, as when `space` isn't installed yet. When `omarchy plugin
// update` changes `manifest.json` to a new version (or the plugin starts ahead
// of `space`), it runs `install.sh --update`, which fetches the matching
// binary if `install.sh` installed the current one.
//
// As the plugin starts, `install.sh --setup` does what's missing of installing
// it, `space` included. Until that's done, there is nothing to run.
Item {
  id: root

  // The version the shell loaded this service with.
  property string loadedVersion: ""
  // The version currently in `manifest.json` on disk.
  property string manifestVersion: root.loadedVersion
  // The version we already tried running `install.sh --update` for, so an
  // offline machine or missing release doesn't loop.
  property string attemptedVersion: ""
  // Whether `space watch` reported a wire-version mismatch.
  property bool wireMismatch: false

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

  readonly property string installScript: String(Qt.resolvedUrl("../install.sh")).replace(/^file:\/\//, "")

  function maybeUpdate() {
    var want = root.manifestVersion
    if (!want || want === root.attemptedVersion || updater.running) return
    if (!root.wireMismatch && !Spaces.needsUpdate(want, root.now.space)) return
    root.attemptedVersion = want
    updater.command = [root.installScript, "--update"]
    updater.running = true
  }

  function ended() {
    root.ready = false
    root.failures++
    if (root.exitCode === null) root.error = setup.running
      ? "Installing the space command…" : setup.error || "The space command isn't installed: run install.sh in the plugin's folder"
    else if (root.failures > 1 && !root.error) root.error = "space watch exited with " + root.exitCode
    again.interval = Math.min(10000, 250 * Math.pow(2, Math.min(root.failures, 6)))
    again.restart()
  }

  FileView {
    path: String(Qt.resolvedUrl("../manifest.json")).replace(/^file:\/\//, "")
    watchChanges: true
    printErrors: false
    onFileChanged: reload()
    onLoaded: {
      var version = Spaces.parseManifestVersion(text())
      if (version) root.manifestVersion = version
      root.maybeUpdate()
    }
  }

  Process {
    id: updater
    stdout: StdioCollector {
      onStreamFinished: {
        if (!text.includes("Installed space ")) return
        // `space watch` exits on its own within a couple seconds when its
        // binary is replaced; stop it now if it was stuck on a wire mismatch.
        if (root.wireMismatch && proc.running) {
          root.failures = 0
          proc.running = false
        }
        if (root.loadedVersion && root.manifestVersion && root.manifestVersion !== root.loadedVersion) {
          Quickshell.execDetached([
            "notify-send",
            "--app-name=Spaces",
            "Spaces updated to " + root.manifestVersion,
            "Run 'omarchy restart shell' to reload the plugin.",
          ])
        }
      }
    }
  }

  Process {
    id: setup
    property string error: ""
    command: [root.installScript, "--setup"]
    running: true
    stdout: SplitParser {
      onRead: function(line) {
        if (!line.startsWith("Installed space ") || proc.running) return
        root.failures = 0
        again.interval = 0
        again.restart()
      }
    }
    stderr: SplitParser {
      onRead: function(line) { setup.error = line.replace(/^install\.sh: /, "") }
    }
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
          root.wireMismatch = false
          root.maybeUpdate()
        } catch (e) {
          root.error = String(e.message || e)
          if (Spaces.isVersionError(e)) {
            root.wireMismatch = true
            root.maybeUpdate()
          }
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
