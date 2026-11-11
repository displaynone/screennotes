# Packaging ScreenNotes

All commands run from the repository root. Packaging does not install the app, modify the running instance, or require root. Output files, SHA-256 checksums, and build metadata are written to `dist/`. The version comes from `Cargo.toml`. Native `amd64` and `arm64` builds are supported; cross-compilation is not supported by these scripts.

## Recommended: build on the Ubuntu 24.04 baseline

Install Podman or Docker, then run:

```bash
bash scripts/package-container.sh deb
bash scripts/package-container.sh appimage
# Or create both formats in one build:
bash scripts/package-container.sh all
```

The container uses Ubuntu 24.04 and Rust 1.94.1. It builds from a read-only source mount and writes packages to `dist/`, owned by your current user. It downloads the base images, build dependencies, Rust crates, and AppImage tooling. The container is discarded after each run; the builder image remains cached. Use `CONTAINER_ENGINE=docker` to choose Docker when both engines are installed. Do not pass `--skip-build` to the container wrapper: it intentionally has no host binary available.

Building on an older baseline avoids accidentally requiring newer glibc or GTK symbols. Packages still need testing on the oldest supported system and your actual monitor setup. Neither format bundles the desktop portal service or a compositor: the host must provide them.

## Build a Debian package locally

On Ubuntu, install the build dependencies:

```bash
sudo apt-get install build-essential pkg-config libgtk-4-dev dpkg-dev desktop-file-utils python3
python3 scripts/package.py deb
```

Python 3.11 or newer and a recent stable Rust toolchain are required. To package the existing `target/release/screennotes` binary without compiling:

```bash
python3 scripts/package.py deb --skip-build
# Optionally use a specific binary and maintainer identity:
python3 scripts/package.py deb --skip-build --binary /path/to/screennotes \
  --maintainer 'Your Name <you@example.com>'
```

The package includes the binary, desktop launcher, color and symbolic icons, PNG icons, license, and README. `dpkg-shlibdeps` calculates library dependencies from the binary; the screenshot portal is added explicitly. The GNOME portal backend and AppIndicator extension are recommended packages. Standard desktop/icon-cache package triggers handle registration, so no custom root installation scripts are needed. Packaging does not enable autostart.

Install the resulting package using APT so dependencies are resolved:

```bash
sudo apt install ./dist/screennotes_0.6.0_amd64.deb
```

Replace the version and architecture with your output filename. If you previously used the per-user installer, its `~/.local/bin/screennotes` and user launcher can take precedence over the system package. Quit the old instance and remove or rename that personal installation before switching to `/usr/bin/screennotes`.

**Local builds target their build system.** For example, a package built on Ubuntu 26.04 may not install on 24.04. Read its `.build.json` or `dpkg-deb --field PACKAGE.deb Depends`; use the container build to target the 24.04 baseline.

## Build an AppImage locally

Use Ubuntu 24.04 with the dependencies above, plus:

```bash
sudo apt-get install curl librsvg2-dev libgirepository1.0-dev \
  libgdk-pixbuf2.0-bin adwaita-icon-theme patchelf squashfs-tools zsync
python3 scripts/package.py appimage
# Or reuse an existing binary built on this same system:
python3 scripts/package.py appimage --skip-build
```

The script downloads checksum-verified linuxdeploy and its GTK plugin into `target/packaging/tools/`. It deploys GTK4, image loaders, schemas, icons, and linked libraries, and removes the plugin's forced X11 setting so GTK can use Wayland. The output is `dist/ScreenNotes-VERSION-x86_64.AppImage` or `...-aarch64.AppImage`.

The current plugin does not deploy the separate Glycin loaders used by GDK Pixbuf 2.44 and later. The script rejects that build environment instead of emitting an image that fails to load screenshots. Use the Ubuntu 24.04 container on newer hosts.

```bash
chmod +x dist/ScreenNotes-0.6.0-x86_64.AppImage
./dist/ScreenNotes-0.6.0-x86_64.AppImage --background
# Without FUSE:
./dist/ScreenNotes-0.6.0-x86_64.AppImage --appimage-extract-and-run --background
```

The AppImage does not install a persistent launcher or enable login startup. It uses the host's portal and StatusNotifierItem support. Graphics drivers, fonts, and the desktop session remain host-provided. Before distributing, test the full capture → annotation → clipboard/save workflow on a clean target machine, not just `--help`.

## Tool pinning and offline builds

`tools.json` records the expected SHA-256 hashes and GTK plugin commit. The linuxdeploy download URL is a moving upstream `continuous` release: if its content changes, the build deliberately fails the checksum check. Review the new upstream release and update the hash before using it. The container base image tags and APT packages are not frozen, so builds are not claimed to be byte-for-byte reproducible.

For an offline build, provide predownloaded tools (and cached Cargo dependencies), or reuse the verified tool cache:

```bash
python3 scripts/package.py appimage --skip-build \
  --linuxdeploy /path/to/linuxdeploy-x86_64.AppImage \
  --gtk-plugin /path/to/linuxdeploy-plugin-gtk.sh
```

Explicit tool overrides are trusted local files and bypass the download checksum checks. `--linuxdeploy` must point to the linuxdeploy AppImage, not an extracted executable. The output plugin may also need its AppImage runtime download cached or supplied through `LDAI_RUNTIME_FILE`.

Run `python3 scripts/package.py --help` for output/work directory options. Generated AppImages include build-system dependency copyright notices under `usr/share/doc/screennotes/bundled-libraries`; review applicable redistribution obligations before publishing.

Tool references: [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy), [GTK plugin](https://github.com/linuxdeploy/linuxdeploy-plugin-gtk), [AppImage output plugin](https://github.com/linuxdeploy/linuxdeploy-plugin-appimage).

## GitHub Actions

The **Build packages** workflow (`.github/workflows/packages.yml`) builds the native
x86_64 Debian package and AppImage using the same Ubuntu 24.04 container as local
builds. It runs on pushes and pull requests to `main` or `master`, on `v*` tags,
and manually from **Actions → Build packages → Run workflow**. The workflow must
be present on the default branch for the manual button to appear.

Each successful run provides two downloadable artifacts, retained for 30 days:

- `screennotes-deb-amd64`: the `.deb`, SHA-256 checksum and build metadata.
- `screennotes-appimage-x86_64`: the AppImage, SHA-256 checksum and build metadata.

Open the completed run and download them from **Artifacts**. After extracting the
AppImage artifact, use `chmod +x ScreenNotes-*.AppImage` before launching it;
artifact ZIP downloads do not preserve executable permissions. The workflow checks
checksums, Debian metadata and both command-line entry points. Desktop capture,
permissions and monitor layout still require testing in a graphical session.

Package versions come from `Cargo.toml`; pushing a tag does not change the version.
Branch and pull-request runs upload build artifacts. Tag runs also publish a GitHub
Release with both installers, checksums and build metadata. No custom secrets are
needed: only the release job receives repository contents write permission. Third-party action versions
are pinned to commits from the official [checkout](https://github.com/actions/checkout)
and [upload-artifact](https://github.com/actions/upload-artifact) repositories.

Generated packages and build directories are ignored by Git. `Cargo.lock`, SVGs
and PNG icons must remain committed: clean CI builds need them. If generated files
were committed before the ignore rules were added, remove those specific paths
from Git's index (for example, `git rm -r --cached dist target`) and commit the
removal; the command preserves the local files. Only name paths actually tracked.

### Publishing a release

Commit and push the workflow and source changes first. Update the version with `python3 scripts/version.py VERSION`, commit the changed
files, then push a matching tag:

```bash
git tag v0.6.0
git push origin v0.6.0
```

Use the current package version. A mismatch fails before compilation. After both
packages pass verification, the release job creates **Releases → ScreenNotes
v0.6.0** and attaches the installers directly under **Assets**. Tags containing a
hyphen are published as prereleases. Branch builds do not create releases. A manual
workflow run against an existing matching tag can also publish a release, provided
that tag contains this workflow. Existing published releases are not overwritten;
use a new version and tag for subsequent releases. Previously completed Actions
runs will not retroactively publish a release.

GitHub **Packages** is separate from Releases and does not provide a Debian/APT or
AppImage registry. These downloads belong in Releases; no Packages entry is created.

## Updating the version

Use Python 3.11 or newer; no Cargo build or network connection is needed:

```bash
python3 scripts/version.py patch --dry-run
python3 scripts/version.py patch
# Alternatively, increment minor/major or choose an explicit version:
python3 scripts/version.py minor
python3 scripts/version.py major
python3 scripts/version.py 0.2.0
python3 scripts/version.py v0.2.0-beta.1
```

These are alternative commands, not a sequence to run together. The script updates
the local package in `Cargo.toml` and `Cargo.lock`, plus download and release-tag
examples in both READMEs. Dependency versions and historical notes are preserved.
It validates semantic versions and checks that the manifest and lockfile agree
before writing. Increment commands produce the next stable version (for example,
`patch` on `0.2.0-beta.1` becomes `0.2.1`); use an explicit version to graduate a
prerelease to `0.2.0`.

Review and commit the changed files before creating the printed release tag.
The script does not build packages, create commits/tags, or push to GitHub.

### Container DNS failures

If APT reports `Could not resolve archive.ubuntu.com` or
`Could not resolve security.ubuntu.com`, the following missing-package errors are
a consequence of the failed index download. GitHub Actions uses
`CONTAINER_NETWORK=host` for both image construction and package generation to
avoid the Docker bridge DNS path. Local builds keep the engine's default network;
on Linux you can opt into the same configuration:

```bash
CONTAINER_NETWORK=host bash scripts/package-container.sh all
```

APT retries downloads three times and stops if any index remains unavailable.
Host networking still requires working DNS and internet connectivity on the host.
After committing a workflow fix, start a new run on the updated commit; re-running
an old run uses its original workflow and source. A release tag must also contain
the fix, so use a new version/tag when publishing the corrected build.
