# Spike: Vim in the browser (Vimium) and in JetBrains IDEs (IdeaVim) (#113)

Written on 2026-10-26 from how each platform works. Neither was installed or run here, so treat this as a design study. The builds are #124 and #125.

## Vimium (Firefox, Chrome)

**What can be seen.** Flow Buddy already sees the keys (evdev) and that the browser has focus (the app id). What it can't tell is whether those keys go to Vimium or to a text field on the page. A small companion extension can, from a content script: whether an editable element has focus (Vimium's insert mode), how far the page scrolled, and tab switches. It sends no URLs, titles or page content. An extension can't read Vimium's own state, but it doesn't need to.

**How it talks to the daemon.** Browser extensions can't open Unix sockets. Native messaging is the standard way out: the browser starts a registered program and talks JSON over its stdin and stdout. `flowbuddy native-host` would bridge that to the daemon's socket. Registration is one manifest file per browser (`~/.mozilla/native-messaging-hosts/`, `~/.config/google-chrome/NativeMessagingHosts/`), which `install.sh` can write.

**Worth coaching.**
- Holding `j` or `k` to scroll far: `d`/`u` for half a page, `gg`/`G`.
- Cycling tabs with `J`/`K` many times: `T` searches tabs by name.
- Reaching for the mouse to click links: `f` shows link hints.

**Context.** A new `browser-vim` context, used only when the companion is connected and no text field has focus.

## IdeaVim (IntelliJ IDEA, PyCharm, …)

**What can be seen.** An IntelliJ Platform plugin can listen to editor focus, caret moves and document changes, and read the text locally to compute the same motion facts as the VS Code companion. IdeaVim's current mode is reachable through its API, though that API has changed between versions, so the plugin should treat it as best effort and fall back to the daemon's own guess.

**How it talks to the daemon.** The same editor protocol over the same Unix socket. The JDK bundled with JetBrains IDEs (17+) supports Unix domain sockets directly.

**Context.** The window class (`jetbrains-idea`, `jetbrains-pycharm`, …) maps to a JetBrains context much like `vscode-vim`, with the plugin's focus reports separating the editor from tool windows (terminal, run, chat).

**Effort.** Larger than the VS Code companion: a Kotlin plugin with a Gradle build in `jetbrains-plugin/`, tested against two or three IDE versions.
