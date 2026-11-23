default:
    @just --list

install:
    cargo install --path .

# Add the sidebar to the Omarchy shell and bind it to super+p
setup:
    ln -sfn {{justfile_directory()}}/omarchy ~/.config/omarchy/plugins/org.ren.renspace
    omarchy-shell shell rescanPlugins
    # The rescan finishes after the call returns; wait until the shell lists the plugin.
    for i in $(seq 50); do omarchy-shell shell listPlugins | grep -q '"org.ren.renspace"' && break; sleep 0.1; done
    omarchy plugin enable org.ren.renspace
    # The tab strip and the sidebar show the workspaces; the bar needn't.
    omarchy plugin disable omarchy.workspaces
    ln -sfn {{justfile_directory()}}/hypr/renspace.lua ~/.config/hypr/renspace.lua
    grep -qxF 'require("hypr.renspace")' ~/.config/hypr/bindings.lua || echo 'require("hypr.renspace")' >> ~/.config/hypr/bindings.lua
