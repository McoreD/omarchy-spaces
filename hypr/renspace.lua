-- renSpace: one key to fuzzy-find a space and jump to it. Super + 1..0 stay
-- Omarchy's; for keys that stay inside the active space, see
-- renspace-workspaces.lua.
--
-- Install: link this file to ~/.config/hypr/renspace.lua and add
--   require("hypr.renspace")
-- at the end of ~/.config/hypr/bindings.lua, which loads after Omarchy's
-- defaults. Remove that line to undo it.

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
