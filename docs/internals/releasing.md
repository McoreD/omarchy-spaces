# Releasing

Work is merged to `main` as it's ready, and a version is released from `main` every so often.

`space` and the plugin share one version, in `Cargo.toml` and `manifest.json`; the tests fail if they differ.

## What users get between releases

`omarchy plugin add` clones the repository's default branch, and `omarchy plugin update` fast-forwards to it, so users get the plugin as it is on `main`, released or not. `install.sh` downloads `space` from the release of the version in `manifest.json`, and only when that version changes. Between releases, then, an updated plugin runs with the last release's `space`.

That's fine while the plugin only uses what that `space` has. When something merged makes the plugin need a newer `space` (a new command, a new field in what `space watch` prints), release soon after, so users don't sit on a plugin that calls what their `space` lacks.

## To release

1. On `main`, run `just bump 0.2.0`. It raises the version in `Cargo.toml`, `Cargo.lock` and `manifest.json`, commits only those files as `Release 0.2.0`, and tags the commit `v0.2.0`. Do it on `main` directly rather than through a pull request, so the new version doesn't reach GitHub before step 3.
2. Push only the tag: `git push upstream v0.2.0`. `.github/workflows/release.yml` runs CI, checks that the tag is the version in `manifest.json`, builds `space` for x86-64 and ARM64 and publishes the GitHub release.
3. Once the release is published with both builds, push `main`.

The order matters. As soon as `main` has the new version, a plugin that updates sees it and runs `install.sh --update` to download that release. If it isn't published yet, the download fails, and the plugin doesn't try again for that version until the shell restarts.
