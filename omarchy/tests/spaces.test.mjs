import { test } from "node:test"
import assert from "node:assert/strict"

import { appName, displayDir, filterRows, initialIndex, locate, parseRows, score, tabsFor } from "../Spaces.mjs"

const json = JSON.stringify([
  { name: "home", dir: null, active: false, windows: 4, workspaces: [1, 2], last: 2, left_at: 1000 },
  { name: "ren", dir: "/home/me/Projects/ren", active: true, windows: 3, workspaces: [1], last: 1, left_at: 3000 },
  { name: "renspace", dir: "/home/me/dev/ren/renspace", active: false, windows: 0, workspaces: [], last: 1, left_at: 2000 },
  { name: "tax-2026", dir: "/home/me/Spaces/tax", active: false, windows: null, workspaces: null, last: 1, left_at: 0 },
])

test("parses space ls --json", () => {
  const rows = parseRows(json)
  assert.equal(rows.length, 4)
  assert.deepEqual(rows[0], { name: "home", dir: "", active: false, windows: 4, leftAt: 1000 })
  assert.equal(rows[3].windows, -1, "no count without Hyprland")
  assert.throws(() => parseRows("space: error"), /unexpected/)
  assert.throws(() => parseRows("{}"), /unexpected/)
  assert.deepEqual(parseRows("[{\"dir\":\"/x\"}]"), [], "rows without a name are dropped")
})

test("shortens home folders", () => {
  assert.equal(displayDir("/home/me/Projects/ren", "/home/me"), "~/Projects/ren")
  assert.equal(displayDir("/home/me", "/home/me"), "~")
  assert.equal(displayDir("/home/mel/x", "/home/me"), "/home/mel/x")
  assert.equal(displayDir("", "/home/me"), "")
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
  const names = (query) => filterRows(rows, query).map((row) => row.name)
  assert.deepEqual(names(""), ["home", "ren", "renspace", "tax-2026"])
  assert.deepEqual(names("  "), ["home", "ren", "renspace", "tax-2026"])
  assert.deepEqual(names("ren"), ["ren", "renspace"])
  assert.deepEqual(names("spc"), ["renspace"])
  assert.deepEqual(names("2026"), ["tax-2026"])
  assert.deepEqual(names("qq"), [])
})

test("starts on the space left most recently", () => {
  const rows = parseRows(json)
  assert.equal(rows[initialIndex(rows)].name, "renspace", "ren is active, so not it")
  const fresh = rows.map((row) => ({ ...row, leftAt: 0 }))
  assert.equal(fresh[initialIndex(fresh)].name, "ren", "nothing left yet: the active one")
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
      { slot: 1, label: "Chromium", windows: 2, active: false },
      { slot: 3, label: "Nvim", windows: 1, active: true },
    ],
  })
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
