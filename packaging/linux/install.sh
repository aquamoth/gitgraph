#!/bin/sh
# Installs parterre for the current user: the binary into ~/.local/bin, and the desktop entry
# and icon where GNOME, KDE and the others find them. Run it after `cargo build --release`,
# or give it the path of a release binary.
#
#   packaging/linux/install.sh [BINARY]     install (default: target/release/parterre)
#   packaging/linux/install.sh --uninstall  remove all of it again
#
# The entry gets the absolute path of the installed binary: a desktop loads an entry only if
# its Exec can be found, and ~/.local/bin is often not on the session's PATH. On Wayland the
# entry is the only source of a window's icon, so a running parterre shows it after a restart.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin=$HOME/.local/bin/parterre
entry=$data/applications/parterre.desktop
hicolor=$data/icons/hicolor
sizes="16 24 32 48 64 128 256 512"

refresh() {
    # Desktops trust an existing icon cache over the directory, so bring it up to date.
    if [ -f "$hicolor/icon-theme.cache" ] && command -v gtk-update-icon-cache >/dev/null; then
        gtk-update-icon-cache -f -t "$hicolor"
    fi
    if [ -d "$data/applications" ] && command -v update-desktop-database >/dev/null; then
        update-desktop-database "$data/applications"
    fi
}

if [ "${1-}" = --uninstall ]; then
    rm -f "$bin" "$entry" "$hicolor/scalable/apps/parterre.svg"
    for s in $sizes; do rm -f "$hicolor/${s}x${s}/apps/parterre.png"; done
    refresh
    echo "removed $bin, $entry and the icon"
    exit 0
fi

src=${1:-$root/target/release/parterre}
if [ ! -x "$src" ]; then
    echo "no binary at $src; run 'cargo build --release' first" >&2
    exit 1
fi
install -Dm755 "$src" "$bin"
mkdir -p "$data/applications"
sed "s|^Exec=parterre |Exec=$bin |" "$here/parterre.desktop" > "$entry"
install -Dm644 "$root/packaging/icon/parterre.svg" "$hicolor/scalable/apps/parterre.svg"
for s in $sizes; do
    install -Dm644 "$root/packaging/icon/parterre-$s.png" "$hicolor/${s}x${s}/apps/parterre.png"
done
refresh
echo "installed $bin, $entry and the icon"
