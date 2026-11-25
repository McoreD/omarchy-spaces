# Releasing

`omarchy plugin add` and `omarchy plugin update` take whatever is on `main`, and `install.sh` takes the release of the version in its `manifest.json`. So `main` has to go with the latest release: keep work that changes both the plugin and `space` on a branch until it's released.

`space` and the plugin share one version, in `Cargo.toml` and `manifest.json`; the tests fail if they differ.

To release:

1. Raise the version in `Cargo.toml`, `Cargo.lock` and `manifest.json`, and commit.
2. Push `main` together with the version's tag, such as `v0.2.0`.

The tag has `.github/workflows/release.yml` run CI, build `space` for x86-64 and ARM64 and make the GitHub release. Until it's done, a few minutes later, `install.sh` says there's no such release and changes nothing.
