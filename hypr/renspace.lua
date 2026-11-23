-- renSpace keybinds: Super + N and Super + Shift + N stay inside the active
-- space. In `home` (the plain numbered workspaces) they behave exactly as
-- Omarchy's defaults, since `space` hands them to Omarchy's own per-monitor
-- workspace logic.
--
-- Install: copy this file to ~/.config/hypr/renspace.lua and add
--   require("hypr.renspace")
-- at the end of ~/.config/hypr/bindings.lua, which loads after Omarchy's
-- defaults. Remove that line to go back to Omarchy's workspaces.

for n = 1, 10 do
  local key = "code:" .. tostring(n + 9)
  o.rebind("SUPER + " .. key, "Switch to workspace " .. n .. " in this space", "space ws " .. n)
  o.rebind("SUPER + SHIFT + " .. key, "Move window to workspace " .. n .. " in this space", "space move " .. n)
  o.rebind(
    "SUPER + SHIFT + ALT + " .. key,
    "Move window silently to workspace " .. n .. " in this space",
    "space move --silent " .. n
  )
end

-- Super + P is Omarchy's pseudo window and Super + Shift + P its Google Photos
-- web app, so the switcher goes on Super + Alt + P (for project).
o.bind("SUPER + ALT + P", "Switch space", "space go")

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
