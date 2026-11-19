# Publishing

A release (push a `v*` tag; see `.github/workflows/release.yml`) builds everything the stores take. These steps need the maintainer's accounts, so they're done by hand.

## Before the first release

- `scripts/check-versions.sh 0.1.0` passes (CI runs it on the tag too).
- `CHANGELOG.md` has a `## [0.1.0]` entry: the release notes come from it. Replace "unreleased" with the date.

## GNOME extensions (extensions.gnome.org)

1. The uuid, `flowbuddy@abdelrahmanwm.github.io`, has to be a domain you control. `abdelrahmanwm.github.io` is your GitHub Pages domain, so it is.
2. Check it the way reviewers do, with [shexli](https://pypi.org/project/shexli/) in a virtualenv: `shexli flowbuddy@abdelrahmanwm.github.io.shell-extension.zip` should say `clean`.
3. Upload `flowbuddy@abdelrahmanwm.github.io.shell-extension.zip` from the release at <https://extensions.gnome.org/upload/>.
4. In the description, say clearly that the extension needs the Flow Buddy daemon (link to docs/install.md). Reviewers reject extensions that look broken on their own.
5. Review rules this extension already meets: everything is created in `enable()` and undone in `disable()`; timeouts are removed; no subprocesses in the shell process (only the preferences window runs `flowbuddy`); no GTK in the shell.

## VS Code Marketplace and Open VSX

1. Create a publisher named `flowbuddy` at <https://marketplace.visualstudio.com/manage> (or change `publisher` in `vscode-extension/package.json` to yours), and an Open VSX namespace at <https://open-vsx.org>.
2. The icon is `vscode-extension/icon.png` (from `docs/images/icon.svg`).
3. Publish the `.vsix` from the release: `npx @vscode/vsce publish --packagePath flowbuddy-vscode-0.1.0.vsix` and `npx ovsx publish flowbuddy-vscode-0.1.0.vsix -p <token>`.

## Vim and Neovim

Nothing to publish: plugin managers install it straight from this repository (see [vim-plugin/README.md](../vim-plugin/README.md)).

## Names

Before publishing, check that "Flow Buddy" is free on each store, and decide what to do if it isn't.
