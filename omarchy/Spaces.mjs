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
