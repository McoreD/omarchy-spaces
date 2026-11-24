#!/usr/bin/env python3
"""Writes omarchy/glyphs.json, the glyphs the icon picker offers, from Nerd
Fonts' glyphnames.json (`just glyphs` downloads it): `[glyph, keywords]`
pairs, the common ones first, then the rest by set, Material Design first.

    python3 scripts/glyphs.py glyphnames.json omarchy/glyphs.json
"""

import json
import sys

# The sets worth an icon; the rest are powerline separators, indent guides
# and the like.
SETS = ["md", "fa", "dev", "linux", "cod", "oct", "seti", "weather", "fae"]

# What the picker shows first, before you search: things a space is likely
# about.
COMMON = """
md-home md-folder md-code_tags md-console md-git md-github md-rocket_launch
md-book_open_variant md-notebook md-pencil md-briefcase md-email md-calendar
md-chat md-music md-movie md-camera md-image md-palette md-brush md-star
md-heart md-fire md-lightning_bolt md-flask md-atom md-school md-cash
md-chart_line md-cart md-controller_classic md-airplane md-car md-bike md-run
md-dumbbell md-food md-coffee md-glass_mug_variant md-pizza md-leaf md-tree
md-flower md-paw md-cat md-dog md-bug md-wrench md-hammer md-cog md-server
md-database md-cloud md-web md-earth md-map md-translate md-lock md-key
md-shield md-account md-account_group md-robot md-brain md-lightbulb
md-puzzle md-trophy md-flag md-bookmark md-tag md-bell md-clock md-timer
md-newspaper md-file_document md-note md-clipboard md-bank md-hospital
md-heart_pulse md-baby_face md-office_building md-store md-truck md-ship_wheel
md-anchor md-compass md-image_filter_hdr md-beach md-weather_sunny md-weather_night
md-snowflake md-umbrella md-gift md-cake md-party_popper md-guitar_acoustic
md-piano md-headphones md-microphone md-video md-television md-cellphone
md-laptop md-monitor md-keyboard md-printer md-package_variant md-cube
md-layers md-shape md-vector_bezier md-draw md-test_tube md-microscope
md-telescope md-calculator md-scale_balance md-gavel md-receipt md-wallet
md-credit_card md-bitcoin md-chart_pie md-target md-bullseye_arrow md-sword
md-ghost md-alien md-skull md-crown md-diamond md-infinity md-yin_yang
md-language_rust md-language_python md-language_javascript
md-language_typescript md-language_go md-language_c md-language_cpp
md-language_ruby md-language_java md-language_swift md-language_haskell
md-language_lua md-language_php md-language_html5 md-language_css3
md-react md-vuejs md-nodejs md-docker md-kubernetes md-linux md-apple
md-microsoft_windows md-android md-arch md-ubuntu dev-vim
md-api md-sitemap md-source_branch md-source_pull md-bash
linux-archlinux linux-nixos linux-tux dev-rust dev-python dev-go dev-postgresql
""".split()


def main(source, target):
    names = json.load(open(source, encoding="utf-8"))
    names.pop("METADATA", None)
    missing = [name for name in COMMON if name not in names]
    if missing:
        print("not in glyphnames.json:", " ".join(missing), file=sys.stderr)

    order = [name for name in COMMON if name in names]
    for prefix in SETS:
        order += sorted(name for name in names if name.split("-", 1)[0] == prefix)

    # A glyph some sets share is listed once, findable by each name.
    keywords = {}
    for name in order:
        prefix, rest = name.split("-", 1)
        words = rest.replace("_", " ").replace("-", " ") + " " + prefix
        char = names[name]["char"]
        if char in keywords:
            if words not in keywords[char]:
                keywords[char] += " " + words
        else:
            keywords[char] = words

    with open(target, "w", encoding="utf-8") as out:
        json.dump([[char, words] for char, words in keywords.items()], out, ensure_ascii=False, separators=(",", ":"))
        out.write("\n")
    print(f"{len(keywords)} glyphs", file=sys.stderr)


if __name__ == "__main__":
    main(*sys.argv[1:3])
