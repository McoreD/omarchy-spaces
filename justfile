default:
    @just --list

install:
    cargo install --path .

bind:
    ln -s ~/dev/ren/renspace/hypr/renspace.lua ~/.config/hypr/renspace.lua
    echo 'require("hypr.renspace")' >> ~/.config/hypr/bindings.lua
