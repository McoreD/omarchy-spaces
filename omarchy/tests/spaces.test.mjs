import { test } from "node:test"
import assert from "node:assert/strict"

import { displayDir, filterRows, initialIndex, parseRows, score } from "../Spaces.mjs"

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
