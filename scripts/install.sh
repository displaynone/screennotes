#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
prefix="${HOME}/.local"
autostart=false
prebuilt=false
for arg in "$@"; do
    case "$arg" in
        --autostart) autostart=true ;;
        --prebuilt) prebuilt=true ;;
        *) echo "Usage: $0 [--autostart] [--prebuilt]" >&2; exit 2 ;;
    esac
done
if [[ "$prebuilt" == false ]]; then
    pkg-config --atleast-version=4.10 gtk4 || {
        echo 'Install the dependencies: sudo apt-get install build-essential pkg-config libgtk-4-dev' >&2
        exit 1
    }
    cargo build --release --locked
fi
test -x target/release/screennotes
install -Dm755 target/release/screennotes "$prefix/bin/screennotes"
install -Dm644 assets/screennotes.svg "$prefix/share/icons/hicolor/scalable/apps/screennotes.svg"
install -Dm644 assets/screennotes-symbolic.svg "$prefix/share/icons/hicolor/symbolic/apps/screennotes-symbolic.svg"
mkdir -p "$prefix/share/applications"
python3 - "$prefix" "$autostart" <<'PY'
from pathlib import Path
import sys
prefix = Path(sys.argv[1])
# Desktop Entry escaping, including paths with spaces or shell metacharacters.
binary = str(prefix / 'bin/screennotes')
binary = binary.replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
quoted = '"' + binary + '"'
entry = Path('assets/io.github.screennotes.ScreenNotes.desktop').read_text().replace('Exec=screennotes ', 'Exec=' + quoted + ' ')
(prefix / 'share/applications/io.github.screennotes.ScreenNotes.desktop').write_text(entry)
if sys.argv[2] == 'true':
    import os
    config = Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config')))
    auto = config / 'autostart/io.github.screennotes.ScreenNotes.desktop'
    auto.parent.mkdir(parents=True, exist_ok=True)
    auto.write_text('[Desktop Entry]\nType=Application\nName=ScreenNotes\nExec=' + quoted + ' --background\nIcon=screennotes\nX-GNOME-Autostart-enabled=true\n')
PY
if command -v update-desktop-database >/dev/null; then update-desktop-database "$prefix/share/applications"; fi
if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -f -t "$prefix/share/icons/hicolor" || true; fi
echo "Installed. Run: $prefix/bin/screennotes --background"
