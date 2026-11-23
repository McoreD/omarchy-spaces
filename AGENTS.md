# AGENTS.md

## Checking visual changes

After changing anything you can see (the QML in `omarchy/`), check it on screen before calling it done:

1. Run `just reload`. It restarts the Omarchy shell, which takes a few seconds. Reloading the shell's plugins isn't enough, because the sidebar lives in the plugin's service, which `keepLoaded` keeps through a plugin reload.
2. Take a screenshot with `grim` into your scratchpad, and look at it. `grim -g "0,0 700x450" <file>` captures the top left, where the sidebar and the tabs are.
3. To see the expanded sidebar, open it before the screenshot and close it after, so the screen is left as it was: `omarchy-shell shell call org.ren.renspace showOrHide ''`, then wait about a second for it to slide open.
