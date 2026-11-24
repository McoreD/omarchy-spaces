-- renSpace for Hyprland. It binds no keys: bind the ones you want in
-- ~/.config/hypr/bindings.lua, after the require (see the README's Keys).
--
-- Install: link this file to ~/.config/hypr/renspace.lua and add
--   local renspace = require("hypr.renspace")
-- at the end of ~/.config/hypr/bindings.lua, which loads after Omarchy's
-- defaults. Remove that line to undo it.

-- renspace.go_to_tab(n) and renspace.move_window_to_tab(n[, follow]) make
-- actions for tab keys. This is plain Lua inside Hyprland rather than a call to
-- `space`, so it can't leave you stuck: outside a space (`home`) each one runs
-- the action Omarchy binds to workspace n, and works even if `space` is
-- missing.

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

local M = { go_to_tab = focus, move_window_to_tab = move_window }

-- A tab emptied by closing its last window doesn't linger, however the
-- window went (super+w, `exit` in a terminal, an app quitting): `space
-- emptied` goes on to another of the space's tabs, or, with none left, ends
-- the space and goes back to where you were before it. Only a close gets
-- here, so an empty workspace you go to yourself stays. The closing window
-- still counts until the next loop, so look then.
hl.on("window.close", function(window)
  local workspace = window and window.workspace
  local name = workspace and workspace.name
  if not name or not name:match("^[a-z0-9][a-z0-9-]*:%d+$") or name:match("^home:") then
    return
  end
  hl.timer(function()
    local active = hl.get_active_workspace()
    if active and active.name == name and active.windows == 0 then
      hl.dispatch(hl.dsp.exec_cmd("space emptied " .. name))
    end
  end, { timeout = 1, type = "oneshot" })
end)

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
-- blur what is behind it. The sidebar follows the bar's look, so blur behind
-- it too. `omarchy bar background` adds or removes this toggle file and
-- reloads Hyprland, which runs this again. With xray the blur takes in only
-- the wallpaper, not the windows next to the sidebar and the bar, which would
-- smear into their edges like a shadow; the bar gets it too.
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

return M
