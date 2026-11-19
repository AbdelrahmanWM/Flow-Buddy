# Publishing

A release (push a `v*` tag; see `.github/workflows/release.yml`) builds everything the stores take. These steps need the maintainer's accounts, so they're done by hand.

## Before a release

- Set the new version in `Cargo.toml`, `gnome-extension/metadata.json` (`version-name`), `vscode-extension/package.json` and `vim-plugin/autoload/flowbuddy.vim`, then `cargo update -w`. `scripts/check-versions.sh X.Y.Z` must pass (CI runs it on the tag too).
- `CHANGELOG.md` has a dated `## [X.Y.Z]` entry: the release notes come from it.
- Merge that, then push the tag `vX.Y.Z` on main.

## GNOME extensions (extensions.gnome.org)

1. The uuid, `flowbuddy@abdelrahmanwm.github.io`, has to be a domain you control. `abdelrahmanwm.github.io` is your GitHub Pages domain, so it is.
2. Check it the way reviewers do, with [shexli](https://pypi.org/project/shexli/) in a virtualenv: `shexli flowbuddy@abdelrahmanwm.github.io.shell-extension.zip` should say `clean`.
3. Upload `flowbuddy@abdelrahmanwm.github.io.shell-extension.zip` from the release at <https://extensions.gnome.org/upload/>.
4. In the description, say clearly that the extension needs the Flow Buddy daemon (link to docs/install.md). Reviewers reject extensions that look broken on their own.
5. Review rules this extension already meets: everything is created in `enable()` and undone in `disable()`; timeouts are removed; no subprocesses in the shell process (only the preferences window runs `flowbuddy`); no GTK in the shell.

## VS Code Marketplace and Open VSX

A release has five `.vsix` files: one per platform with the daemon and the `flowbuddy` command inside (`linux-x64`, `linux-arm64`, `darwin-x64`, `darwin-arm64`), and one without, for every other system. Upload all five: the stores give each system its own, and the one without to the rest.

1. The publisher is `flowbuddy` (`publisher` in `vscode-extension/package.json`), and the Open VSX namespace `flowbuddy`.
2. The icon is `vscode-extension/icon.png` (from `docs/images/icon.svg`).
3. By hand: on the Marketplace, your publisher page, then the extension's **⋯** → **Update**, once per file. On Open VSX, `npx ovsx publish <file> -p <token>` for each.
4. Or let the release do it: add the repository secrets `VSCE_PAT` (an Azure DevOps token with **Marketplace → Manage**, for all accessible organizations) and `OVSX_PAT` (an Open VSX access token). The release workflow then publishes every `.vsix` to both.

## Vim and Neovim

Nothing to publish: plugin managers install it straight from this repository (see [vim-plugin/README.md](../vim-plugin/README.md)), and `scripts/get-daemon.sh` downloads the daemon from the release whose version matches the plugin's. So the version in `vim-plugin/autoload/flowbuddy.vim` must have its release published before people update to it: bump it in the release PR, as the other versions.

## Names

Before publishing, check that "Flow Buddy" is free on each store, and decide what to do if it isn't.
