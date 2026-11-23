default:
    @just --list

install:
    cargo install --path .

# Add the tabs to the bar and the sidebar to the Omarchy shell, and bind the keys
setup:
    ln -sfn {{justfile_directory()}}/omarchy ~/.config/omarchy/plugins/org.ren.renspace
    omarchy-shell shell rescanPlugins
    # The rescan finishes after the call returns; wait until the shell lists the plugin.
    for i in $(seq 50); do omarchy-shell shell listPlugins | grep -q '"org.ren.renspace"' && break; sleep 0.1; done
    omarchy plugin enable org.ren.renspace
    # The space's tabs take the place of the bar's workspaces, after the menu.
    omarchy plugin disable omarchy.workspaces
    omarchy bar put org.ren.renspace --after omarchy.menu
    ln -sfn {{justfile_directory()}}/hypr/renspace.lua ~/.config/hypr/renspace.lua
    grep -qxF 'require("hypr.renspace")' ~/.config/hypr/bindings.lua || echo 'require("hypr.renspace")' >> ~/.config/hypr/bindings.lua

# Restart the Omarchy shell, so it picks up changes to the QML. Reloading its
# plugins isn't enough: it keeps keepLoaded services, which the sidebar is.
reload:
    omarchy restart shell
