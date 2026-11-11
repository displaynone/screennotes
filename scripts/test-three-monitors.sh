#!/usr/bin/env bash
# Isolated GNOME/Mutter session with three virtual Wayland outputs.
# Requires gnome-shell, dbus-run-session and an already compiled test executable.
set -euo pipefail
if [[ "${1:-}" != --inside ]]; then
    test_bin="${1:?Usage: bash scripts/test-three-monitors.sh /absolute/path/to/test-binary}"
    runtime="$(mktemp -d /tmp/screennotes-wayland.XXXXXX)"
    chmod 700 "$runtime"
    echo "GNOME test logs: $runtime"
    exec env XDG_RUNTIME_DIR="$runtime" GSETTINGS_BACKEND=memory LIBGL_ALWAYS_SOFTWARE=1 \
        dbus-run-session -- bash "$0" --inside "$test_bin"
fi
test_bin="$2"
export WAYLAND_DISPLAY=screennotes-test
export GDK_BACKEND=wayland
unset DISPLAY
gnome-shell --headless --wayland --no-x11 --wayland-display "$WAYLAND_DISPLAY" \
    --virtual-monitor 1920x1080 --virtual-monitor 1600x900 --virtual-monitor 1280x1024 \
    >"$XDG_RUNTIME_DIR/gnome-shell.log" 2>&1 &
shell_pid=$!
trap 'kill "$shell_pid" 2>/dev/null || true; wait "$shell_pid" 2>/dev/null || true' EXIT
for ((i=0;i<100;i++)); do
    if [[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]]; then break; fi
    if ! kill -0 "$shell_pid" 2>/dev/null; then cat "$XDG_RUNTIME_DIR/gnome-shell.log"; exit 1; fi
    sleep 0.1
done
SCREENNOTES_EXPECT_MONITORS=3 timeout 20 "$test_bin" editor::tests::desktop_editor_smoke --ignored --nocapture
