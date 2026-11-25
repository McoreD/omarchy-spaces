default:
    @just --list

install:
    cargo install --path .

# Add the tabs to the bar and the sidebar to the Omarchy shell, and load the Hyprland part
setup:
    ln -sfn {{justfile_directory()}} ~/.config/omarchy/plugins/com.t4t5.spaces
    omarchy-shell shell rescanPlugins
    # The rescan finishes after the call returns; wait until the shell lists the plugin.
    for i in $(seq 50); do omarchy-shell shell listPlugins | grep -q '"com.t4t5.spaces"' && break; sleep 0.1; done
    # First on the bar, the space's logo and tabs take the place of the menu and the workspaces.
    omarchy plugin enable com.t4t5.spaces --section left --index 0
    omarchy plugin disable omarchy.menu
    omarchy plugin disable omarchy.workspaces
    ln -sfn {{justfile_directory()}}/hypr/omarchy-spaces.lua ~/.config/hypr/omarchy-spaces.lua
    grep -q 'require("hypr.omarchy-spaces")' ~/.config/hypr/bindings.lua || echo 'local spaces = require("hypr.omarchy-spaces")' >> ~/.config/hypr/bindings.lua

# Make the icon picker's glyphs again, from a Nerd Fonts release's glyph names
glyphs version="v3.4.0":
    mkdir -p target
    curl -sSfL -o target/glyphnames.json https://raw.githubusercontent.com/ryanoasis/nerd-fonts/{{version}}/glyphnames.json
    python3 scripts/glyphs.py target/glyphnames.json omarchy/glyphs.json

# Restart the Omarchy shell, so it picks up changes to the QML. Reloading its
# plugins isn't enough: it keeps keepLoaded services, which the sidebar is.
reload:
    omarchy restart shell

# Go between spaces quickly, here, and check that the sidebar and bars keep up
stress rounds="20":
    python3 scripts/stress.py {{rounds}}
