#!/usr/bin/env python3
"""Goes between spaces quickly, on the desktop you are on, and checks that the
sidebar and the bars show where Hyprland says you are each time it settles.
You end up where you were. `just stress [rounds]`."""

import json
import subprocess
import sys
import time

PLUGIN = "com.t4t5.spaces"
SETTLE = 1.0


def run(*command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def locate(workspace):
    """The space and tab a workspace is, as `space` reads its name."""
    space, _, key = workspace["name"].rpartition(":")
    if space:
        return space, int(key)
    return "home", max(workspace["id"], 0)


def hyprland():
    monitors = json.loads(run("hyprctl", "monitors", "-j"))
    where = {m["name"]: locate(m["activeWorkspace"]) for m in monitors}
    focused = next(m["name"] for m in monitors if m["focused"])
    return focused, where


def shell():
    said = run("omarchy-shell", "shell", "call", PLUGIN, "shown", "")
    shown = json.loads(said)
    where = {m["name"]: (m["space"], m["key"]) for m in shown["monitors"]}
    return shown["space"], shown["focused"], where


def check(what):
    """Whether they agree, and how long that took."""
    start = time.monotonic()
    while True:
        focused, where = hyprland()
        space, shown_focused, shown = shell()
        if space == [where[focused][0]] and shown_focused == focused and shown == where:
            return time.monotonic() - start
        if time.monotonic() - start > SETTLE:
            print(f"{what}: Hyprland has {focused} {where}")
            print(f"{what}: the shell has {shown_focused} {shown}, the sidebar {space}")
            return None


def main():
    rounds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    spaces = json.loads(run("space", "ls", "--json"))
    if len(spaces) < 2:
        sys.exit("There's only one space to go to.")
    start = hyprland()
    failed, slowest = 0, 0.0
    try:
        for i in range(rounds):
            gap = (i % 8) * 0.02
            there, back = ("next", "prev") if i % 2 else ("prev", "next")
            # Up to three on, then as many back, from one at a time to all at
            # once, as keypresses come.
            steps = [there] * (i % 3 + 1) + [back] * (i % 3 + 1)
            running = []
            for step in steps:
                running.append(subprocess.Popen(["space", step]))
                time.sleep(gap)
            for step in running:
                step.wait()
            took = check(f"round {i}, {gap * 1000:.0f} ms between {len(steps)} steps")
            if took is None:
                failed += 1
            else:
                slowest = max(slowest, took)
    finally:
        was = start[1][start[0]][0]
        run("space", "go", was)
    time.sleep(0.5)
    if check("at the end") is None:
        failed += 1
    if hyprland() != start:
        print(f"You were on {start}, and are on {hyprland()}")
    print(f"{rounds} rounds, {failed} wrong; agreed within {slowest * 1000:.0f} ms at most")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
