-- renSpace, optional: Super + 1..0 and Super + Shift (+ Alt) + 1..0 stay inside
-- the active space, so a space can have workspaces of its own.
--
-- This is plain Lua inside Hyprland, not a call to the `space` binary, so it
-- can't leave you stuck: outside a space (`home`), every key runs Omarchy's own
-- action unchanged, the very function Omarchy binds to it, and it works even if
-- `space` is missing or broken. Inside a space, `super+3` goes to `name:3`.
-- `space go home` (or the picker) leads back to the plain workspaces.
--
-- Install: link this file to ~/.config/hypr/renspace-workspaces.lua and add
--   require("hypr.renspace-workspaces")
-- after require("hypr.renspace") in ~/.config/hypr/bindings.lua. Remove that
-- line to undo it.

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
  o.rebind("SUPER + " .. key, "Switch to workspace " .. n .. " in this space", focus(n))
  o.rebind("SUPER + SHIFT + " .. key, "Move window to workspace " .. n .. " in this space", move_window(n))
  o.rebind(
    "SUPER + SHIFT + ALT + " .. key,
    "Move window silently to workspace " .. n .. " in this space",
    move_window(n, false)
  )
end
