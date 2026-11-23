-- renSpace keys. Each space's workspaces are its tabs, shown in the tab strip
-- along the top (the org.ren.renspace shell plugin in ../omarchy).
--
-- Install: link this file to ~/.config/hypr/renspace.lua and add
--   require("hypr.renspace")
-- at the end of ~/.config/hypr/bindings.lua, which loads after Omarchy's
-- defaults. Remove that line to undo it.

-- Super + P (for project) replaces Omarchy's pseudo window toggle. Without the
-- plugin, bind "space go" here instead for the centered picker.
o.rebind("SUPER + P", "Switch space", { panel = "org.ren.renspace" })

-- Tabs, like a browser's. Super + T was Omarchy's floating toggle; Super + Tab
-- cycled every workspace on the monitor, leaving the space.
o.rebind("SUPER + T", "New tab", "space ws new")
o.rebind("SUPER + TAB", "Next tab", "space ws next")
o.rebind("SUPER + SHIFT + TAB", "Previous tab", "space ws prev")

-- Super + 1..0 go to tab N, and Super + Shift (+ Alt) + 1..0 move the focused
-- window there. This is plain Lua inside Hyprland rather than a call to
-- `space`, so it can't leave you stuck: outside a space (`home`) each key runs
-- the very action Omarchy binds to it, and works even if `space` is missing.

-- The space the focused workspace belongs to, or nil in `home`. The same rule
-- as `space`: `name:n`, with a lowercase name and n from 1 to 10.
local function active_space()
  local workspace = hl.get_active_workspace()
  if not workspace or not workspace.name then
    return nil
  end
  local space, slot = workspace.name:match("^([a-z0-9][a-z0-9-]*):(%d+)$")
  slot = tonumber(slot)
  if not space or space == "home" or not slot or slot < 1 or slot > 10 then
    return nil
  end
  return space
end

local function focus(slot)
  local omarchy = o.workspaces.focus(slot)
  return function()
    local space = active_space()
    if not space then
      return omarchy()
    end
    hl.dispatch(hl.dsp.focus({ workspace = "name:" .. space .. ":" .. slot }))
  end
end

local function move_window(slot, follow)
  local omarchy = o.workspaces.move_window(slot, follow)
  return function()
    local space = active_space()
    if not space then
      return omarchy()
    end
    if hl.get_active_window() then
      hl.dispatch(hl.dsp.window.move({ workspace = "name:" .. space .. ":" .. slot, follow = follow ~= false }))
    end
  end
end

for n = 1, 10 do
  local key = "code:" .. tostring(n + 9)
  o.rebind("SUPER + " .. key, "Go to tab " .. n, focus(n))
  o.rebind("SUPER + SHIFT + " .. key, "Move window to tab " .. n, move_window(n))
  o.rebind("SUPER + SHIFT + ALT + " .. key, "Move window silently to tab " .. n, move_window(n, false))
end

-- On a config reload, Omarchy's per-monitor workspace logic
-- (default/hypr/monitor-workspaces.lua) puts every monitor back on a numbered
-- workspace, which drops you out of your space. Hyprland re-reads this file
-- before it reports the reload, so note the workspace now and go back to it
-- once Omarchy has had its turn.
local ok, before = pcall(hl.get_active_workspace)
local before_name = ok and before and before.name or nil

if before_name and before_name:match("^[a-z0-9][a-z0-9-]*:%d+$") then
  hl.on("config.reloaded", function()
    local now = hl.get_active_workspace()
    if not now or now.name ~= before_name then
      hl.dispatch(hl.dsp.focus({ workspace = "name:" .. before_name }))
    end
  end)
end

-- A glassy bar (double-click it to cycle opaque, glassy, clear) has Hyprland
-- blur what is behind it. The base (tab strip and sidebar) follows its look,
-- so blur behind it too. `omarchy bar background` adds or removes this
-- toggle file and reloads Hyprland, which runs this again. With xray the blur
-- takes in only the wallpaper, not the windows next to the base and the bar,
-- which would smear into their edges like a shadow; the bar gets it too.
local glassy = io.open(require("default.hypr.paths").state_home .. "/omarchy/toggles/hypr/bar-glassy.lua")
if glassy then
  glassy:close()
  hl.layer_rule({
    match = { namespace = "^(renspace-base|omarchy-bar)$" },
    blur = true,
    xray = true,
    ignore_alpha = 0.3,
  })
end
