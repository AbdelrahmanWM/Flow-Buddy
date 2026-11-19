#!/bin/sh
# Download Flow Buddy's programs, flowbuddyd and flowbuddy, into bin/ of
# this folder: the release that matches this checkout's version, for this
# system, with its checksum checked. For editor plugin managers, which run
# it as the plugin's build step (see vim-plugin/README.md):
#
#   scripts/get-daemon.sh
#
# Nothing else is installed: no service, no keyboard access. The editor
# plugins find bin/flowbuddyd here and start it when they need it, as in
# an editor-only install. scripts/install.sh uses these binaries too.
set -eu

repo=$(cd "$(dirname "$0")/.." && pwd)
bin=$repo/bin
version=$(sed -n "s/^let s:VERSION = '\(.*\)'/\1/p" "$repo/vim-plugin/autoload/flowbuddy.vim")
[ -n "$version" ] || { echo "get-daemon: can't tell this checkout's version" >&2; exit 1; }

case $(uname -s) in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    *) echo "get-daemon: Flow Buddy runs on Linux and macOS, not $(uname -s)" >&2; exit 1 ;;
esac
case $(uname -m) in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) echo "get-daemon: no build for $(uname -m)" >&2; exit 1 ;;
esac

if [ -x "$bin/flowbuddyd" ] && [ "$("$bin/flowbuddyd" --version 2>/dev/null)" = "flowbuddyd $version" ]; then
    echo "get-daemon: flowbuddyd $version is already in $bin"
    exit 0
fi

name=flowbuddy-v$version-$os-$arch
url=https://github.com/AbdelrahmanWM/Flow-Buddy/releases/download/v$version/$name.tar.gz
# Inside this folder, never a shared one like /tmp.
work=$bin/.download
rm -rf "$work"
mkdir -p "$work"
trap 'rm -rf "$work"' EXIT

fetch() {
    if command -v curl >/dev/null; then
        curl --proto '=https' --tlsv1.2 -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null; then
        wget -q --https-only -O "$2" "$1"
    else
        echo "get-daemon: needs curl or wget" >&2
        exit 1
    fi
}

echo "get-daemon: downloading $name"
fetch "$url" "$work/$name.tar.gz" || { echo "get-daemon: couldn't download $url" >&2; exit 1; }
fetch "$url.sha256" "$work/$name.tar.gz.sha256" || { echo "get-daemon: couldn't download its checksum" >&2; exit 1; }
want=$(cut -d' ' -f1 "$work/$name.tar.gz.sha256")
if command -v sha256sum >/dev/null; then
    got=$(sha256sum "$work/$name.tar.gz" | cut -d' ' -f1)
else
    got=$(shasum -a 256 "$work/$name.tar.gz" | cut -d' ' -f1)
fi
[ -n "$want" ] && [ "$want" = "$got" ] || { echo "get-daemon: the checksum doesn't match; nothing installed" >&2; exit 1; }

tar -xzf "$work/$name.tar.gz" -C "$work" "$name/bin/flowbuddyd" "$name/bin/flowbuddy"
for p in flowbuddyd flowbuddy; do
    mv -f "$work/$name/bin/$p" "$bin/$p"
    chmod 755 "$bin/$p"
done
echo "get-daemon: flowbuddyd and flowbuddy $version are in $bin"
