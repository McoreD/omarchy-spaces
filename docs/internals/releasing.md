# Releasing

`omarchy plugin add` and `omarchy plugin update` take whatever is on `main`, and `install.sh` takes the release of the version in its `manifest.json`. So `main` has to go with the latest release: keep work that changes both the plugin and `space` on a branch until it's released.

`space` and the plugin share one version, in `Cargo.toml` and `manifest.json`; the tests fail if they differ.

To release:

1. Raise the version in `Cargo.toml`, `Cargo.lock` and `manifest.json`, and commit on a release branch.
2. Push the version's tag, such as `v0.2.0`, and wait for `.github/workflows/release.yml` to run CI, build `space` for x86-64 and ARM64 and publish the GitHub release.
3. Fast-forward `main` to that commit, so `omarchy plugin update` always finds the release binary ready to download.
