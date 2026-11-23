// The sidebar's logic, kept out of QML so Node can test it. Everything here
// works on the rows `space ls --json` prints.

// Rows from `space ls --json`, or throws with a readable message.
export function parseRows(text) {
  let rows
  try {
    rows = JSON.parse(String(text || ""))
  } catch (e) {
    throw new Error("space ls --json printed something unexpected")
  }
  if (!Array.isArray(rows)) throw new Error("space ls --json printed something unexpected")
  return rows
    .filter((row) => row && typeof row.name === "string" && row.name !== "")
    .map((row) => ({
      name: row.name,
      dir: typeof row.dir === "string" ? row.dir : "",
      active: row.active === true,
      windows: typeof row.windows === "number" ? row.windows : -1,
      leftAt: typeof row.left_at === "number" ? row.left_at : 0,
    }))
}

// `/home/me/Projects/ren` as `~/Projects/ren`.
export function displayDir(dir, home) {
  if (!dir) return ""
  if (home && home !== "/") {
    if (dir === home) return "~"
    if (dir.startsWith(home + "/")) return "~" + dir.slice(home.length)
  }
  return dir
}

// How well `query` matches `name`: higher is better, -1 for no match. A
// prefix beats a match at a word start (after `-`), which beats a substring,
// which beats the letters in order with gaps.
export function score(query, name) {
  const q = query.toLowerCase()
  const n = name.toLowerCase()
  if (!q) return 0
  if (n.startsWith(q)) return 400 - n.length
  const at = n.indexOf(q)
  if (at > 0 && n[at - 1] === "-") return 300 - n.length
  if (at > 0) return 200 - at
  let from = 0
  let gaps = 0
  for (const c of q) {
    const found = n.indexOf(c, from)
    if (found < 0) return -1
    gaps += found - from
    from = found + 1
  }
  return 100 - gaps
}

// The rows matching `query`, best first. With no query, every row in the
// order `space ls` gives (home, then by name), so each space keeps its place.
export function filterRows(rows, query) {
  const q = String(query || "").trim()
  if (!q) return rows.slice()
  return rows
    .map((row, index) => ({ row, index, score: score(q, row.name) }))
    .filter((entry) => entry.score >= 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.row)
}

// A valid space name made from what you type: `My Trip` is `my-trip`. The
// same rule as `space` (src/name.rs).
export function slugify(raw) {
  let out = ""
  for (const c of String(raw || "").toLowerCase()) {
    if ((c >= "a" && c <= "z") || (c >= "0" && c <= "9")) out += c
    else if (out && !out.endsWith("-")) out += "-"
  }
  return out.replace(/-+$/, "").slice(0, 64)
}

// The filtered rows, and after them a row offering to make a space of the
// query's name when no space has it: `{ name, create: true }`. Activating it
// runs `space new <name> --in-roots`.
export function withNewRow(shown, rows, query) {
  const name = slugify(String(query || "").trim())
  if (!name || name === "home" || rows.some((row) => row.name === name)) return shown
  return shown.concat([{ name, dir: "", active: false, windows: -1, leftAt: 0, create: true }])
}

// Which of a row's actions (its right-click menu) apply, and why not when
// one doesn't. `home` and the new-space row have none.
export function rowActions(row) {
  if (!row || row.create || row.name === "home") return []
  const windows = row.windows > 0 ? row.windows : 0
  return [
    { id: "rename", label: "Rename", enabled: true, hint: "" },
    {
      id: "close",
      label: windows === 1 ? "Close 1 window" : "Close " + (windows || "its") + " windows",
      enabled: windows > 0,
      hint: windows > 0 ? "" : "None open",
    },
    {
      id: "remove",
      label: "Remove space",
      enabled: windows === 0 && !!row.dir,
      hint: !row.dir ? "No folder" : windows > 0 ? "Close windows first" : "Keeps the folder",
    },
  ]
}

// The new name while renaming, after typing `text`: only what a space name
// takes, uppercase lowered and spaces or `_` as `-`, never leading.
export function renameInput(name, text) {
  let out = name
  for (const c of String(text || "").toLowerCase()) {
    const ch = c === " " || c === "_" ? "-" : c
    if ((ch >= "a" && ch <= "z") || (ch >= "0" && ch <= "9") || (ch === "-" && out)) out += ch
  }
  return out.slice(0, 64)
}

// Where the cursor starts: on the space you left most recently, so opening
// the sidebar and pressing Enter goes back. Failing that, the active one.
export function initialIndex(rows) {
  let best = -1
  for (let i = 0; i < rows.length; i++) {
    if (rows[i].active || rows[i].leftAt <= 0) continue
    if (best < 0 || rows[i].leftAt > rows[best].leftAt) best = i
  }
  if (best >= 0) return best
  const active = rows.findIndex((row) => row.active)
  return active >= 0 ? active : 0
}

// ---------------------------------------------------------------- tabs
//
// A space's workspaces are its tabs: `ren:1`, `ren:2`, ... The rules match
// `space` (src/name.rs): a strict `name:n`, anything else is `home`, whose
// tabs are the plain numbered workspaces of the monitor's block of ten.

const SLOTS = 10
const SPACE_WORKSPACE = /^([a-z0-9][a-z0-9-]*):(\d+)$/

// `{ space, slot }` for a workspace; slot is 0 when it has none.
export function locate(id, name) {
  const match = SPACE_WORKSPACE.exec(String(name || ""))
  if (match && match[1] !== "home") {
    const slot = Number(match[2])
    if (String(slot) === match[2] && slot >= 1 && slot <= SLOTS) return { space: match[1], slot }
  }
  return { space: "home", slot: id > 0 ? ((id - 1) % SLOTS) + 1 : 0 }
}

// A short app name from a window class: `org.omarchy.quick-terminal` is
// `Quick-terminal`, Omarchy's `chrome-app.hey.com__-Default` web apps are
// `Hey.com`.
export function appName(windowClass) {
  let name = String(windowClass || "")
  const webApp = /^(?:chrome|chromium|brave)-(.+?)__/.exec(name)
  if (webApp) name = webApp[1].replace(/^(app|www)\./, "")
  else if (name.includes(".")) name = name.slice(name.lastIndexOf(".") + 1)
  return name ? name[0].toUpperCase() + name.slice(1) : ""
}

// The active space's tabs from Hyprland's workspaces and windows.
//   workspaces: [{ id, name }]
//   windows:    [{ workspaceId, windowClass, focus }]  (focus: lower is more recent)
//   active:     { id, name } of the focused workspace
// Returns { space, tabs: [{ slot, label, windows, active }] } in slot order.
export function tabsFor(workspaces, windows, active) {
  const here = locate(active ? active.id : 0, active ? active.name : "")
  const block = (id) => Math.floor((id - 1) / SLOTS)
  const activeBlock = block(active && active.id > 0 ? active.id : 1)

  const tabs = new Map()
  const tabOf = (workspace) => {
    const where = locate(workspace.id, workspace.name)
    if (where.space !== here.space || where.slot === 0) return null
    if (where.space === "home" && block(workspace.id) !== activeBlock) return null
    return where.slot
  }

  for (const workspace of workspaces || []) {
    const slot = tabOf(workspace)
    if (slot === null || tabs.has(slot)) continue
    tabs.set(slot, { id: workspace.id, slot, label: "", windows: 0, active: false, focus: Infinity })
  }
  if (here.slot > 0 && !tabs.has(here.slot)) {
    tabs.set(here.slot, { id: active.id, slot: here.slot, label: "", windows: 0, active: false, focus: Infinity })
  }

  const byId = new Map([...tabs.values()].map((tab) => [tab.id, tab]))
  for (const window of windows || []) {
    const tab = byId.get(window.workspaceId)
    if (!tab) continue
    tab.windows++
    const focus = typeof window.focus === "number" ? window.focus : Infinity
    // The most recently focused window names the tab.
    if (!tab.label || focus < tab.focus) {
      tab.label = appName(window.windowClass)
      tab.focus = focus
    }
  }

  return {
    space: here.space,
    tabs: [...tabs.values()]
      .sort((a, b) => a.slot - b.slot)
      .map((tab) => ({
        slot: tab.slot,
        label: tab.label || "New tab",
        windows: tab.windows,
        active: tab.slot === here.slot,
      })),
  }
}

// ---------------------------------------------------------------- motion
//
// The sidebar moves with the windows it pushes aside, on Hyprland's own
// curve and duration, so no gap opens between them.

// Hyprland's curve when an animation names `default`.
const DEFAULT_BEZIER = [0, 0.75, 0.15, 1]

// How Hyprland moves windows, from `hyprctl animations -j`:
// `{ duration (ms), bezier: [x0, y0, x1, y1] }`, with duration 0 when the
// animation is off, or null when the output can't be read. `windowsMove`
// inherits from `windows`, which inherits from `global`, unless overridden.
export function windowMotion(text) {
  let data
  try {
    data = JSON.parse(String(text || ""))
  } catch (e) {
    return null
  }
  if (!Array.isArray(data) || !Array.isArray(data[0])) return null
  const animations = new Map(data[0].map((a) => [a.name, a]))
  const curves = Array.isArray(data[1]) ? data[1] : []

  let leaf = null
  for (const name of ["windowsMove", "windows", "global"]) {
    const animation = animations.get(name)
    if (animation && (animation.overridden || name === "global")) {
      leaf = animation
      break
    }
  }
  if (!leaf) return null
  if (!leaf.enabled) return { duration: 0, bezier: DEFAULT_BEZIER }

  const curve = curves.find((c) => c.name === leaf.bezier)
  return {
    duration: Math.round(Number(leaf.speed) * 100),
    bezier: curve ? [curve.X0, curve.Y0, curve.X1, curve.Y1] : DEFAULT_BEZIER,
  }
}

// The bar's background, as Omarchy's shell config holds it: opaque, glassy
// (see-through, blurred behind) or none (clear). Double-clicking the bar steps
// through them. A config from before `background` says `transparent` instead.
export function barLook(config) {
  const look = config && config.background
  if (look === "opaque" || look === "glassy" || look === "none") return look
  return config && config.transparent === true ? "none" : "opaque"
}

// The rows from the last time the sidebar was open, brought up to date with
// the space you are in now, so the cursor starts in the right place before
// `space ls` answers: `here` is the active one, and the one that was active
// before was left just now.
export function freshen(rows, here, now) {
  return rows.map((row) => {
    const active = row.name === here
    const leftAt = row.active && !active ? now : row.leftAt
    // Not `{ ...row }`: QML's JavaScript has no object spread, and Node does.
    return Object.assign({}, row, { active, leftAt })
  })
}
