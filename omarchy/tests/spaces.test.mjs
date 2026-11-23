import { test } from "node:test"
import assert from "node:assert/strict"

import { appName, badgeColor, barToggle, fairCap, barLook, DEFAULT_PALETTE, monogram, themePalette, ease, filterRows, freshen, gapLeft, initialIndex, locate, nameInput, parseRows, recessAlpha, rowActions, score, stepAction, statusColors, tabsFor, terminalsByAddress, moreUrgent, windowMotion } from "../Spaces.mjs"

const json = JSON.stringify([
  { id: "home", label: "home", named: false, folder: null, active: false, windows: 4, workspaces: [1, 2], last: 2, left_at: 1000 },
  { id: "s1", label: "ren", named: false, folder: "/home/me/Projects/ren", active: true, windows: 3, workspaces: [1], last: 1, left_at: 3000 },
  { id: "s2", label: "renspace", named: false, folder: "/home/me/dev/ren/renspace", active: false, windows: 0, workspaces: [], last: 1, left_at: 2000 },
  { id: "s3", label: "Tax 2026", named: true, folder: "/home/me/tax", active: false, windows: null, workspaces: null, last: 1, left_at: 0 },
])

test("parses space ls --json", () => {
  const rows = parseRows(json)
  assert.equal(rows.length, 4)
  assert.deepEqual(rows[0], { id: "home", label: "home", named: false, folder: "", active: false, windows: 4, leftAt: 1000, status: "", tabs: {}, terminals: [] })
  assert.equal(rows[3].windows, -1, "no count without Hyprland")
  assert.equal(rows[3].named, true)
  assert.throws(() => parseRows("space: error"), /unexpected/)
  assert.throws(() => parseRows("{}"), /unexpected/)
  assert.deepEqual(parseRows("[{\"folder\":\"/x\"}]"), [], "rows without an id are dropped")
  assert.equal(parseRows("[{\"id\":\"s9\"}]")[0].label, "s9", "no label: the id")
})

test("scores prefix over word start over substring over letters in order", () => {
  assert.ok(score("ren", "ren") > score("ren", "renspace"))
  assert.ok(score("ren", "renspace") > score("tax", "my-taxes"))
  assert.ok(score("tax", "my-tax") > score("tax", "mytax"))
  assert.ok(score("tax", "mytax") > score("tx", "tax-2026"))
  assert.equal(score("rsp", "renspace") > 0, true)
  assert.equal(score("zz", "renspace"), -1)
  assert.equal(score("REN", "ren") > 0, true, "case-insensitive")
})

test("filters and ranks, keeping the list order with no query", () => {
  const rows = parseRows(json)
  const names = (query) => filterRows(rows, query).map((row) => row.label)
  assert.deepEqual(names(""), ["home", "ren", "renspace", "Tax 2026"])
  assert.deepEqual(names("  "), ["home", "ren", "renspace", "Tax 2026"])
  assert.deepEqual(names("ren"), ["ren", "renspace"])
  assert.deepEqual(names("spc"), ["renspace"])
  assert.deepEqual(names("2026"), ["Tax 2026"])
  assert.deepEqual(names("tax"), ["Tax 2026"], "ignoring case")
  assert.deepEqual(names("qq"), [])
})

test("starts on the space left most recently", () => {
  const rows = parseRows(json)
  assert.equal(rows[initialIndex(rows)].label, "renspace", "ren is active, so not it")
  const fresh = rows.map((row) => ({ ...row, leftAt: 0 }))
  assert.equal(fresh[initialIndex(fresh)].label, "ren", "nothing left yet: the active one")
  assert.equal(initialIndex([]), 0)
})

test("locates workspaces like space does", () => {
  assert.deepEqual(locate(-1337, "ren:3"), { space: "ren", slot: 3 })
  assert.deepEqual(locate(-1338, "tax-2026:10"), { space: "tax-2026", slot: 10 })
  assert.deepEqual(locate(13, "13"), { space: "home", slot: 3 })
  for (const name of ["chat", "Ren:1", "ren:0", "ren:11", "ren:01", "home:1"]) {
    assert.deepEqual(locate(-5, name), { space: "home", slot: 0 }, name)
  }
})

test("names apps from window classes", () => {
  assert.equal(appName("chromium"), "Chromium")
  assert.equal(appName("org.omarchy.quick-terminal"), "Quick-terminal")
  assert.equal(appName("com.mitchellh.ghostty"), "Ghostty")
  assert.equal(appName("chrome-app.hey.com__-Default"), "Hey.com")
  assert.equal(appName(""), "")
})

test("builds the active space's tabs", () => {
  const workspaces = [
    { id: 1, name: "1" },
    { id: -1337, name: "ren:1" },
    { id: -1338, name: "ren:3" },
    { id: -1339, name: "tax:1" },
  ]
  const windows = [
    { workspaceId: -1337, windowClass: "kitty", focus: 4 },
    { workspaceId: -1337, windowClass: "chromium", focus: 1 },
    { workspaceId: -1338, windowClass: "nvim", focus: 2 },
    { workspaceId: 1, windowClass: "Slack", focus: 0 },
  ]
  assert.deepEqual(tabsFor(workspaces, windows, { id: -1338, name: "ren:3" }), {
    space: "ren",
    tabs: [
      { slot: 1, label: "Chromium", named: false, status: "", windows: 2, active: false },
      { slot: 3, label: "Nvim", named: false, status: "", windows: 1, active: true },
    ],
  })
})

test("a name you gave a tab wins over its window's", () => {
  const rows = parseRows(JSON.stringify([
    { id: "ren", tabs: { 3: "server", 0: "bad", 4: 7 } },
  ]))
  assert.deepEqual(rows[0].tabs, { 3: "server" })
  const workspaces = [{ id: -1337, name: "ren:1" }, { id: -1338, name: "ren:3" }]
  const windows = [
    { workspaceId: -1337, windowClass: "kitty", focus: 1 },
    { workspaceId: -1338, windowClass: "nvim", focus: 0 },
  ]
  const tabs = tabsFor(workspaces, windows, { id: -1338, name: "ren:3" }, {}, rows[0].tabs).tabs
  assert.deepEqual(tabs.map((t) => [t.label, t.named]), [["Kitty", false], ["server", true]])
})

test("names a terminal's tab by what it runs, else its folder", () => {
  const rows = parseRows(JSON.stringify([
    { id: "home", terminals: [{ address: "0xa", slot: 1, folder: "/home/me", program: null, label: "~" }] },
    { id: "s1", terminals: [
      { address: "0xb", slot: 1, folder: "/p/ren", program: "claude", label: "claude" },
      { address: "0xc", slot: 2, folder: "/p/ren/src", program: null, label: "src" },
      { address: 7, label: "broken" },
    ] },
  ]))
  const labels = terminalsByAddress(rows)
  assert.deepEqual(Object.keys(labels), ["0xa", "0xb", "0xc"])
  assert.equal(labels["0xb"].label, "claude")

  const workspaces = [{ id: -1337, name: "s1:1" }, { id: -1338, name: "s1:2" }, { id: -1339, name: "s1:3" }]
  const windows = [
    { workspaceId: -1337, address: "0xb", windowClass: "kitty", focus: 0 },
    { workspaceId: -1338, address: "0xc", windowClass: "kitty", focus: 1 },
    // Not a terminal, or not read yet: its class.
    { workspaceId: -1339, address: "0xd", windowClass: "kitty", focus: 2 },
  ]
  const tabs = tabsFor(workspaces, windows, { id: -1337, name: "s1:1" }, labels).tabs
  assert.deepEqual(tabs.map((t) => t.label), ["claude", "src", "Kitty"])
  assert.equal(tabsFor(workspaces, windows, { id: -1337, name: "s1:1" }).tabs[0].label, "Kitty", "no labels: classes")
})

test("shows what most needs you among a tab's and a space's agents", () => {
  const rows = parseRows(JSON.stringify([
    { id: "s1", status: "waiting", terminals: [
      { address: "0xa", label: "claude", status: "working" },
      { address: "0xb", label: "claude", status: "waiting" },
      { address: "0xc", label: "claude", status: "done" },
      { address: "0xd", label: "nvim", status: null },
      { address: "0xe", label: "x", status: "bogus" },
    ] },
    { id: "s2", status: "asleep" },
  ]))
  assert.equal(rows[0].status, "waiting")
  assert.equal(rows[1].status, "", "an unknown status is idle")
  assert.equal(rows[0].terminals[4].status, "")

  const workspaces = [{ id: -1, name: "s1:1" }, { id: -2, name: "s1:2" }, { id: -3, name: "s1:3" }]
  const windows = [
    { workspaceId: -1, address: "0xa", windowClass: "kitty", focus: 0 },
    { workspaceId: -1, address: "0xb", windowClass: "kitty", focus: 1 },
    { workspaceId: -2, address: "0xc", windowClass: "kitty", focus: 2 },
    { workspaceId: -2, address: "0xa1", windowClass: "chromium", focus: 3 },
    { workspaceId: -3, address: "0xd", windowClass: "kitty", focus: 4 },
  ]
  const tabs = tabsFor(workspaces, windows, { id: -1, name: "s1:1" }, terminalsByAddress(rows)).tabs
  assert.deepEqual(tabs.map((t) => t.status), ["waiting", "done", ""])

  assert.equal(moreUrgent("working", "done"), "done")
  assert.equal(moreUrgent("waiting", "done"), "waiting")
  assert.equal(moreUrgent("", "working"), "working")
  assert.deepEqual(statusColors('red = "#F7768E"\ngreen = "#9ece6a"\n'), { waiting: "#f7768e", done: "#9ece6a" })
  assert.deepEqual(statusColors(""), { waiting: "", done: "" })
})

test("a new tab is empty until something opens on it", () => {
  const tabs = tabsFor([{ id: -1337, name: "ren:1" }], [], { id: -1340, name: "ren:2" })
  assert.deepEqual(tabs.tabs.map((t) => [t.slot, t.label, t.active]), [
    [1, "New tab", false],
    [2, "New tab", true],
  ])
})

test("home's tabs are the focused monitor's plain workspaces", () => {
  const workspaces = [{ id: 1, name: "1" }, { id: 2, name: "2" }, { id: 12, name: "12" }, { id: -1337, name: "ren:1" }]
  const tabs = tabsFor(workspaces, [], { id: 2, name: "2" })
  assert.equal(tabs.space, "home")
  assert.deepEqual(tabs.tabs.map((t) => t.slot), [1, 2])
  assert.deepEqual(tabsFor(workspaces, [], { id: 12, name: "12" }).tabs.map((t) => t.slot), [2])
})

test("reads Hyprland's window motion", () => {
  const curves = [{ name: "easeOutQuint", X0: 0.23, Y0: 1, X1: 0.32, Y1: 1 }]
  const animations = (overrides) => JSON.stringify([
    [
      { name: "global", overridden: true, bezier: "default", enabled: true, speed: 10 },
      { name: "windows", overridden: true, bezier: "easeOutQuint", enabled: true, speed: 3.79 },
      { name: "windowsMove", overridden: false, bezier: "", enabled: true, speed: 0 },
      ...overrides,
    ],
    curves,
  ])
  assert.deepEqual(windowMotion(animations([])), { duration: 379, bezier: [0.23, 1, 0.32, 1] })

  const moveOverride = animations([{ name: "windowsMove", overridden: true, bezier: "default", enabled: true, speed: 2 }])
  // A later entry wins in the Map, as Hyprland lists each leaf once.
  assert.deepEqual(windowMotion(moveOverride), { duration: 200, bezier: [0, 0.75, 0.15, 1] })

  const off = JSON.stringify([[{ name: "global", overridden: true, bezier: "default", enabled: false, speed: 10 }], []])
  assert.equal(windowMotion(off).duration, 0)
  assert.equal(windowMotion("hyprctl: error"), null)
})

test("reads the left of Hyprland's outer gaps", () => {
  assert.equal(gapLeft('{"option": "general:gaps_out", "css": "10 20 30 40", "set": true}'), 40)
  assert.equal(gapLeft('{"option": "general:gaps_out", "css": "0 0 0 0", "set": true}'), 0)
  assert.equal(gapLeft('{"option": "general:gaps_out", "custom": "5 6 7 8"}'), 8)
  assert.equal(gapLeft('{"css": "10 20"}'), 20, "two values: vertical, then horizontal")
  assert.equal(gapLeft('{"css": "12"}'), 12)
  assert.equal(gapLeft('{"int": 7}'), 7)
  assert.equal(gapLeft('{"css": "a b c d"}'), null)
  assert.equal(gapLeft("hyprctl: no such option"), null)
})

test("marks a space with two letters, as T3 Code marks a project", () => {
  assert.equal(monogram("rencal"), "RL")
  assert.equal(monogram("caldir"), "CR")
  assert.equal(monogram("Tax 2026"), "T2")
  assert.equal(monogram("omarchy-caldir-widget"), "OW")
  assert.equal(monogram("v2ray"), "V2")
  assert.equal(monogram("my notes"), "MN")
  assert.equal(monogram("x"), "X")
  assert.equal(monogram("ångström"), "ÅM")
  assert.equal(monogram("~"), "?")
  assert.equal(monogram(""), "?")
})

test("colours a badge by its space's name", () => {
  const palette = ["#111111", "#222222", "#333333"]
  assert.equal(badgeColor("rencal", palette), badgeColor("rencal", palette), "the same each time")
  assert.equal(badgeColor("Rencal", palette), badgeColor("rencal", palette), "whatever its case")
  assert.ok(palette.includes(badgeColor("caldir", palette)))
  assert.equal(badgeColor("caldir", []), "")
})

test("takes badge colours from the theme", () => {
  const toml = [
    'mode = "dark"',
    'accent = "#faa968"',
    'red = "#F85525"',
    'yellow = "#e97b3c"',
    'orange = "#faa968"',
    'green = "#028391"',
    'blue = "#3f8f8a"',
    'magenta = "#3f8f8a"',
    'bright_red = "#ff0000"',
  ].join("\n")
  assert.deepEqual(themePalette(toml), ["#f85525", "#faa968", "#e97b3c", "#028391", "#3f8f8a"])
  assert.deepEqual(themePalette('red = "#ff0000"'), DEFAULT_PALETTE, "too few of its own")
  assert.deepEqual(themePalette(""), DEFAULT_PALETTE)
})

test("shares the bar's room between tabs, short ones whole", () => {
  assert.equal(fairCap([50, 60], 200), Infinity, "they fit")
  assert.equal(fairCap([40, 100, 100], 200), 80, "the short one keeps its width")
  assert.equal(fairCap([100, 100], 100), 50)
  assert.equal(fairCap([], 10), Infinity)
})

test("eases along a cubic Bézier as Hyprland does", () => {
  const linear = [0, 0, 1, 1]
  assert.ok(Math.abs(ease(linear, 0.25) - 0.25) < 1e-9)
  assert.equal(ease(linear, -1), 0)
  assert.equal(ease(linear, 2), 1)
  // easeOutQuint, as Omarchy sets it: most of the way there early on.
  const quint = [0.23, 1, 0.32, 1]
  assert.ok(Math.abs(ease(quint, 0.5) - 0.9660) < 1e-3, String(ease(quint, 0.5)))
  // CSS's ease at its midpoint.
  assert.ok(Math.abs(ease([0.25, 0.1, 0.25, 1], 0.5) - 0.8024) < 1e-3, String(ease([0.25, 0.1, 0.25, 1], 0.5)))
  let last = 0
  for (let x = 0; x <= 1; x += 0.01) {
    const y = ease(quint, x)
    assert.ok(y >= last - 1e-9, "never goes back")
    last = y
  }
})

test("sets the background back more on a dark bar than a light one", () => {
  assert.equal(recessAlpha(0.03, 0.08, 0.15), 0.3)
  assert.equal(recessAlpha(0.95, 0.93, 0.9), 0.07)
})

test("reads the bar's background look", () => {
  assert.equal(barLook({ background: "glassy" }), "glassy")
  assert.equal(barLook({ background: "none" }), "none")
  assert.equal(barLook({ transparent: true }), "none", "the older on/off form")
  assert.equal(barLook({ background: "opaque", transparent: true }), "opaque")
  assert.equal(barLook({ background: "neon" }), "opaque")
  assert.equal(barLook(null), "opaque")
})

test("reads whether the sidebar's header is on the bar", () => {
  const id = "org.ren.renspace"
  assert.equal(barToggle({ layout: { left: [{ id: "omarchy.menu" }, { id }] } }, id), false)
  assert.equal(barToggle({ layout: { left: [{ id, toggle: true }] } }, id), true)
  assert.equal(barToggle({ position: "bottom", layout: { left: [{ id, toggle: true }] } }, id), true)
  assert.equal(barToggle({ position: "left", layout: { left: [{ id, toggle: true }] } }, id), false, "a vertical bar")
  assert.equal(barToggle({ layout: { right: [{ id, toggle: true }] } }, id), false, "only the left section lines up")
  assert.equal(barToggle({ layout: { left: [null, { id, toggle: "yes" }] } }, id), false)
  assert.equal(barToggle(null, id), false)
})

test("freshens the last rows for the space you are in now", () => {
  const rows = parseRows(json)
  // The last list was taken in ren (s1); since then you went to renspace (s2).
  const fresh = freshen(rows, "s2", 9000)
  assert.deepEqual(fresh.map((row) => row.active), [false, false, true, false])
  assert.equal(fresh[1].leftAt, 9000, "ren was left just now")
  assert.equal(fresh[0].leftAt, 1000)
  assert.equal(rows[initialIndex(fresh)].label, "ren", "the cursor starts on the space you came from")
  assert.equal(rows[initialIndex(rows)].label, "renspace", "the stale rows would start on the space you are in")
  assert.deepEqual(freshen(rows, "s1", 9000), rows, "nothing changed while it was closed")
})

test("offers rename and close, saying why close doesn't apply", () => {
  const rows = parseRows(json)
  const actions = (row) => rowActions(row).map((a) => [a.id, a.enabled, a.hint])
  assert.deepEqual(rowActions(rows[0]), [], "home has none")
  assert.deepEqual(actions(rows[1]), [["rename", true, ""], ["close", true, "3 windows"]])
  assert.deepEqual(actions(rows[2])[1], ["close", false, "No windows"])
  assert.equal(rowActions({ id: "s4", windows: 1 })[1].hint, "1 window")
  assert.equal(rowActions(rows[3])[1].enabled, false, "no count without Hyprland")
  assert.deepEqual(rowActions(rows[1]).map((a) => a.separatorBefore), [false, true])
})

test("steps through the enabled actions", () => {
  const actions = [{ enabled: true }, { enabled: true }, { enabled: false }]
  assert.equal(stepAction(actions, -1, 1), 0, "down from nothing: the first")
  assert.equal(stepAction(actions, -1, -1), 1, "up from nothing: the last enabled")
  assert.equal(stepAction(actions, 1, 1), 0, "skips the disabled one and wraps")
  assert.equal(stepAction(actions, 0, -1), 1)
  assert.equal(stepAction([{ enabled: false }], -1, 1), -1)
  assert.equal(stepAction([], -1, 1), -1)
})

test("takes any name up to 64 characters", () => {
  assert.equal(nameInput("Japan", "ese"), "Japanese")
  assert.equal(nameInput("", "Tax 2026"), "Tax 2026")
  assert.equal(nameInput("日本", "語"), "日本語")
  assert.equal(nameInput("x".repeat(64), "y"), "x".repeat(64))
})
