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
    .filter((row) => row && typeof row.id === "string" && row.id !== "")
    .map((row) => ({
      id: row.id,
      label: typeof row.label === "string" && row.label ? row.label : row.id,
      named: row.named === true,
      folder: typeof row.folder === "string" ? row.folder : "",
      active: row.active === true,
      windows: typeof row.windows === "number" ? row.windows : -1,
      leftAt: typeof row.left_at === "number" ? row.left_at : 0,
    }))
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

// The rows matching `query` by what they're called, best first. With no
// query, every row in the order `space ls` gives (home, then the order they
// were made in), so each space keeps its place.
export function filterRows(rows, query) {
  const q = String(query || "").trim()
  if (!q) return rows.slice()
  return rows
    .map((row, index) => ({ row, index, score: score(q, row.label) }))
    .filter((entry) => entry.score >= 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.row)
}

// A row's right-click menu: which actions apply, and why not when one
// doesn't. `home` has none. Icons are Nerd Font glyphs: a pencil, a cross.
export function rowActions(row) {
  if (!row || row.id === "home") return []
  const windows = row.windows > 0 ? row.windows : 0
  return [
    { id: "rename", icon: "\u{f0cb6}", label: "Rename", enabled: true, hint: "", separatorBefore: false },
    {
      id: "close",
      icon: "\u{f0156}",
      label: "Close space",
      enabled: windows > 0,
      hint: windows === 1 ? "1 window" : windows > 1 ? windows + " windows" : "No windows",
      separatorBefore: true,
    },
  ]
}

// The next enabled action going `by` (1 or -1) from `from`, wrapping; from
// nothing (-1), the first or the last. -1 when none is enabled.
export function stepAction(actions, from, by) {
  const count = actions.length
  let index = from < 0 ? (by > 0 ? -1 : count) : from
  for (let i = 0; i < count; i++) {
    index = (index + by + count) % count
    if (actions[index].enabled) return index
  }
  return -1
}

// A name being typed, after typing `text`: anything printable, up to 64
// characters, as `space rename` takes.
export function nameInput(name, text) {
  return Array.from(String(name || "") + String(text || "")).slice(0, 64).join("")
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

// The left of Hyprland's outer gaps, from `hyprctl getoption general:gaps_out
// -j`: the windows' distance from what reserves the screen's edge. Hyprland
// prints the four sides as CSS does (`css`, or `custom` before it), or one
// number for all (`int`). Null when the output can't be read.
export function gapLeft(text) {
  let data
  try {
    data = JSON.parse(String(text || ""))
  } catch (e) {
    return null
  }
  if (!data || typeof data !== "object") return null
  if (typeof data.int === "number") return Math.max(0, data.int)
  const sides = String(data.css ?? data.custom ?? "").trim().split(/\s+/).map(Number)
  if (sides.length === 0 || sides.some((n) => !Number.isFinite(n))) return null
  // CSS order: top, right, bottom, left; fewer values repeat as CSS repeats them.
  const left = sides.length === 4 ? sides[3] : sides.length >= 2 ? sides[1] : sides[0]
  return Math.max(0, left)
}

// A space's badge: two letters for its name, as T3 Code marks its projects.
// The first letter, then the first digit after it in the first word, or the
// last word's first letter, or the first word's last letter: `rencal` is RL,
// `Tax 2026` is T2, `my notes` is MN. A one-letter name keeps its one letter.
//
// The shell's JavaScript has no Unicode classes (`\p{L}`), so a word is a run
// of anything but spaces and ASCII punctuation, and a digit an ASCII one.
export function monogram(name) {
  const text = String(name || "")
  const words = (text.normalize ? text.normalize("NFKC") : text).trim().match(/[^\s!-/:-@[-`{-~]+/g) || []
  if (words.length === 0) return "?"
  const glyphs = Array.from(words[0])
  const first = glyphs[0]
  const digit = glyphs.slice(1).find((glyph) => /[0-9]/.test(glyph))
  let second = digit
  if (!second && words.length > 1) second = Array.from(words[words.length - 1])[0]
  if (!second && glyphs.length > 1) second = glyphs[glyphs.length - 1]
  return Array.from((first + (second || "")).toUpperCase()).slice(0, 2).join("")
}

// The colour of a space's badge, from `palette`, by its name: the same name
// always takes the same colour.
export function badgeColor(name, palette) {
  if (!palette || palette.length === 0) return ""
  const text = String(name || "")
  const seed = (text.normalize ? text.normalize("NFKC") : text).trim().toLowerCase() || "space"
  let index = 0
  for (const glyph of seed) index = (index * 31 + glyph.codePointAt(0)) % palette.length
  return palette[index]
}

// What badges are coloured with when the theme doesn't say.
export const DEFAULT_PALETTE = ["#f87171", "#fb923c", "#facc15", "#4ade80", "#2dd4bf", "#60a5fa", "#a78bfa", "#f472b6"]

// The colours for badges from an Omarchy theme's colors.toml: its hues, each
// once, in a fixed order, or the default palette when it names too few.
export function themePalette(text) {
  const colors = {}
  for (const line of String(text || "").split("\n")) {
    const match = line.match(/^\s*([a-z_]+)\s*=\s*"(#[0-9a-fA-F]{6})"/)
    if (match) colors[match[1]] = match[2].toLowerCase()
  }
  const palette = []
  for (const hue of ["red", "orange", "yellow", "green", "cyan", "blue", "magenta"]) {
    if (colors[hue] && !palette.includes(colors[hue])) palette.push(colors[hue])
  }
  return palette.length >= 3 ? palette : DEFAULT_PALETTE
}

// The widest any tab may be for `widths` (each tab's own) to fit in `room`:
// tabs narrower than an equal share keep their width, and the rest split
// what is left. Infinity when they all fit.
export function fairCap(widths, room) {
  const sorted = widths.slice().sort((a, b) => a - b)
  let left = room
  for (let i = 0; i < sorted.length; i++) {
    const share = left / (sorted.length - i)
    if (sorted[i] > share) return share
    left -= sorted[i]
  }
  return Infinity
}

// How far along a cubic Bézier easing is at `x` (0 to 1) of its time, as
// Hyprland and CSS ease: `bezier` is [x1, y1, x2, y2], the curve running from
// (0, 0) to (1, 1). It solves the curve's x for its parameter, by Newton's
// method and bisection where that stalls, then reads its y there.
export function ease(bezier, x) {
  if (x <= 0) return 0
  if (x >= 1) return 1
  const [x1, y1, x2, y2] = bezier
  const at = (a, b, t) => 3 * a * t * (1 - t) * (1 - t) + 3 * b * t * t * (1 - t) + t * t * t
  const slope = (a, b, t) => 3 * a * (1 - t) * (1 - t) + 6 * (b - a) * t * (1 - t) + 3 * (1 - b) * t * t
  let t = x
  for (let i = 0; i < 8; i++) {
    const d = slope(x1, x2, t)
    if (Math.abs(d) < 1e-6) break
    const next = t - (at(x1, x2, t) - x) / d
    if (next < 0 || next > 1) break
    t = next
  }
  if (Math.abs(at(x1, x2, t) - x) > 1e-5) {
    let lo = 0
    let hi = 1
    for (let i = 0; i < 40; i++) {
      t = (lo + hi) / 2
      if (at(x1, x2, t) < x) lo = t
      else hi = t
    }
  }
  return at(y1, y2, t)
}

// How dark a shade lies over the bar's colour where the background shows,
// beside the space's windows and tabs: over the sidebar, and over the bar
// above it. Given the bar's colour (0 to 1 a channel), a light one takes a
// lighter shade, which is enough to set it back.
export function recessAlpha(r, g, b) {
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
  return luminance > 0.5 ? 0.07 : 0.3
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
// `space ls` answers: `here` (an id) is the active one, and the one that was
// active before was left just now.
export function freshen(rows, here, now) {
  return rows.map((row) => {
    const active = row.id === here
    const leftAt = row.active && !active ? now : row.leftAt
    // Not `{ ...row }`: QML's JavaScript has no object spread, and Node does.
    return Object.assign({}, row, { active, leftAt })
  })
}
