# ScreenNotes

Capture a region and annotate it over a frozen desktop on Ubuntu GNOME with Wayland. Built with Rust and GTK4. Screenshots use the XDG portal, and the top-bar indicator uses StatusNotifierItem, compatible with Ubuntu AppIndicators.

## Build and run

Requires a recent stable Rust toolchain and GTK ≥ 4.10 (Ubuntu 24.04 or later).

```bash
sudo apt-get install build-essential pkg-config libgtk-4-dev
cargo run -- --background
```

The top-bar indicator provides **Capture region**, help, and quit actions. You can also run `cargo run -- --capture`. The app uses a single instance: subsequent invocations are forwarded to the existing process.

```bash
# Install to ~/.local and enable the indicator at login:
bash scripts/install.sh --autostart
```

Without `--autostart`, the installer only installs the binary, icon, and desktop launcher. Set up a custom Ubuntu keyboard shortcut with the command `/home/YOUR_USERNAME/.local/bin/screennotes --capture`.

If a compiled binary already exists at `target/release/screennotes`, `bash scripts/install.sh --prebuilt` installs it without requiring development libraries.

## Build distributable packages

```bash
python3 scripts/package.py deb
# Build both formats on the Ubuntu 24.04 baseline with Podman or Docker:
bash scripts/package-container.sh all
```

Outputs are written to `dist/`. See [Packaging](packaging/README.md) for dependencies, AppImage builds, and installation instructions.

## Editing

- Drag to select a region. To span several screens, you can also click the first corner and then the second corner on another monitor.
- Choose **Line**, **Rectangle**, or **Text**. To add text, enter it in the toolbar field, then click inside the selection to place it.
- Choose a color from the palette or enter a CSS color (`#RRGGBB`) and press Enter. Adjust line width, font family, and font size. Styles apply to subsequent annotations.
- A toolbar appears on each monitor. The status bar shows the shared active tool and style. **↕** moves the toolbar to the top or bottom to uncover obscured areas.
- **Ctrl+S** or **Ctrl+C** copies the rendered image to the clipboard and closes the overlay. Ctrl+C keeps its usual behavior inside a text field.
- **Ctrl+Shift+S** or **Save** opens the PNG file chooser.
- **Ctrl+Z / Ctrl+Shift+Z** undoes / redoes annotations. **Esc** cancels.
- The app stays running in the top bar to preserve the image in the Wayland clipboard. Quitting completely may lose the clipboard contents if no clipboard manager is running.

All application-owned text is in English. System permission dialogs, native file chooser controls, and messages supplied by GTK or the desktop may follow the system language.

After selecting a region, drag any of its four square corner handles to resize the crop.
The opposite corner stays fixed, annotations stay in place, and the handles are not
included in the copied or saved image. Handles work with any annotation tool selected.

Use **Move up / Move down** to reposition the toolbar, or press **F6** on the active monitor.

## Multiple monitors and Wayland

The app enumerates all GDK monitors and requests a fullscreen surface on each one. Annotations use logical desktop coordinates, supporting negative origins, vertical arrangements, and gaps between screens. Export crops to the original image bounds and preserves its resolution. Gaps between monitors inside a selection retain the pixels provided by the compositor.

The portal must return an image of the **entire desktop** at a uniform scale. GNOME controls permissions and may show a system dialog. No private APIs are used to bypass permissions. If the image does not match the desktop geometry, the app reports an error instead of stretching it. This detects many partial captures, but the portal does not provide enough metadata to identify every crop with the same aspect ratio. Mixed scaling depends on how the compositor assembles the image and must be validated on the actual setup. Universal compatibility with other compositors is not guaranteed.

Connecting or disconnecting a monitor during editing cancels the session. The session is also canceled if GNOME rejects a fullscreen surface. The indicator requires the **Ubuntu AppIndicators** extension, included with Ubuntu; plain GNOME needs a StatusNotifierItem provider.

GitHub Actions also builds both packages automatically. Matching version tags publish the installers under **Releases**. Download them from **Actions → Build packages → Artifacts**, or trigger a manual run. See [packaging instructions](packaging/README.md#github-actions) for triggers and details.

To bump the package version, run `python3 scripts/version.py patch` (or `minor`,
`major`, or an explicit version). Add `--dry-run` to preview the changes.

## Verification

```bash
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
# Geometry and history tests, without installing GTK:
cargo test --no-default-features --locked
# Try the interface without capturing real content:
cargo run -- --demo
```

The optional `desktop_editor_smoke` graphical test briefly opens synthetic images on every screen, checks their geometry, and exports an annotation. Run it with `cargo test desktop_editor_smoke -- --ignored --nocapture`. To test three outputs without changing the current session, build with `cargo test --no-run` and pass the absolute path of the `src/main.rs` test executable to `bash scripts/test-three-monitors.sh PATH`. The script creates an isolated GNOME session with three virtual monitors at different resolutions.

Manual checks on three monitors:

1. Start with `--background` and check the indicator menu.
2. Capture and check that each screen matches its background image, including mixed scaling.
3. Select a region on each monitor, then test a selection across two monitors using two clicks.
4. Add text with accented characters, lines, and rectangles. Change font, size, width, and color; undo and redo.
5. Copy with Ctrl+S and paste into another app. Verify the crop boundaries and that the toolbar is excluded.
6. Save a PNG, cancel a save, cancel a capture, and capture again.
7. Change the monitor layout or disconnect a screen and check that the session is canceled safely.

This first version does not support moving or resizing existing annotations or persisting preferences. Toolbars share the active style and text across monitors.

## Screenshot portal errors

A portal response of `Other` does not identify the underlying cause. It can mean a saved denial, a failed permission dialog, or a backend screenshot failure. On GNOME, check screenshot access in Settings → Apps, if available. Run the app as your desktop user, without `sudo`.

Immediately after reproducing the error, run:

```bash
screennotes --diagnose
# Or use the AppImage you launched:
./ScreenNotes-0.4.0-x86_64.AppImage --diagnose
```

This prints desktop information, portal versions, saved screenshot permissions, and recent portal logs. It does not change permissions or restart services. Review the output before sharing it. Missing optional backends and an absent permission entry are not necessarily errors. Avoid resetting the entire permission store: unidentified host applications may share an entry.

### GNOME launcher / indicator permission recovery

GNOME 50 can fail to display its first screenshot permission dialog when a launcher or top-bar capture has no focused ScreenNotes window (`focus_app is null`). Terminal launches can behave differently because the portal may identify them as an unnamed host application, with a separate saved permission.

On a generic portal failure in GNOME, ScreenNotes now opens a small authorization window. Click **Continue**, then **Allow** in the system dialog. The authorization screenshot is discarded; ScreenNotes closes the helper and takes a fresh screenshot for the editor. Successful later captures open the editor directly. Closing the helper cancels; a failed authorization is reported without a retry loop. This does not modify the permission store directly or grant permission automatically.

Upstream focus check: [GNOME Shell 50 AccessDialog](https://github.com/GNOME/gnome-shell/blob/50.0/js/ui/accessDialog.js).

### Permission request responsiveness (0.1.2)

The portal connection now uses one persistent asynchronous runtime: ashpd caches its D-Bus connection, so destroying a per-capture runtime could leave subsequent requests waiting on stopped I/O tasks. Authorization runs separately from the command queue, so Quit remains available. The helper can be closed while a response is pending, and portal requests time out after 60 seconds. Closing the helper cancels the local capture; dismiss any remaining system permission dialog before retrying.

Portal regression checks (run separately in a graphical desktop session):

```bash
cargo test portal_connection_survives_successive_requests -- --ignored --nocapture
cargo test permission_helper_cancels_while_portal_is_pending -- --ignored --nocapture
```

The first check reads appearance settings three times over the cached portal connection without capturing the screen. The second briefly opens the helper and verifies cancellation against a deliberately nonresponding test backend.
