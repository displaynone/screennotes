#!/usr/bin/env python3
"""Build native Debian packages and GTK4 AppImages without root privileges."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
APP_ID = "io.github.screennotes.ScreenNotes"


def run(*args, **kwargs):
    kwargs.setdefault("check", True)
    return subprocess.run([str(a) for a in args], **kwargs)


def output(*args, **kwargs):
    return run(*args, stdout=subprocess.PIPE, text=True, **kwargs).stdout.strip()


def require(*commands):
    missing = [c for c in commands if not shutil.which(c)]
    if missing:
        raise RuntimeError("Missing build tools: " + ", ".join(missing))


def install(source, destination, mode=0o644):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    destination.chmod(mode)


def stage(binary, directory):
    install(binary, directory / "usr/bin/screennotes", 0o755)
    install(ROOT / f"assets/{APP_ID}.desktop", directory / f"usr/share/applications/{APP_ID}.desktop")
    for kind, name in [("scalable", "screennotes"), ("symbolic", "screennotes-symbolic")]:
        install(ROOT / f"assets/{name}.svg", directory / f"usr/share/icons/hicolor/{kind}/apps/{name}.svg")
    for size in [16, 24, 32, 48, 64, 128, 256, 512]:
        install(ROOT / f"assets/previews/screennotes-{size}.png", directory / f"usr/share/icons/hicolor/{size}x{size}/apps/screennotes.png")
    install(ROOT / "LICENSE", directory / "usr/share/doc/screennotes/copyright")
    install(ROOT / "README.md", directory / "usr/share/doc/screennotes/README.md")
    run("desktop-file-validate", directory / f"usr/share/applications/{APP_ID}.desktop")


def checksum(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def record(artifact, details):
    artifact.with_name(artifact.name + ".sha256").write_text(f"{checksum(artifact)}  {artifact.name}\n")
    metadata = {"version": details.pop("version"), "build_os": platform.freedesktop_os_release(), **details}
    artifact.with_name(artifact.name + ".build.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Created: {artifact}", flush=True)


def deb(binary, version, arch, args, work):
    require("dpkg-deb", "dpkg-shlibdeps")
    directory = work / "deb-root"
    stage(binary, directory)
    source = work / "debian-source"
    (source / "debian").mkdir(parents=True)
    (source / "debian/control").write_text(
        f"Source: screennotes\nSection: graphics\nPriority: optional\nMaintainer: {args.maintainer}\n"
        "\nPackage: screennotes\nArchitecture: any\nDescription: Screenshot annotation tool\n"
    )
    variables = output("dpkg-shlibdeps", "-O", f"-e{binary}", cwd=source)
    dependencies = next((line.split("=", 1)[1] for line in variables.splitlines() if line.startswith("shlibs:Depends=")), None)
    if not dependencies:
        raise RuntimeError("dpkg-shlibdeps did not return runtime dependencies; refusing an incomplete package.")
    dependencies += ", xdg-desktop-portal"
    size = (sum(p.stat().st_size for p in directory.rglob("*") if p.is_file()) + 1023) // 1024
    control = directory / "DEBIAN"
    control.mkdir()
    (control / "control").write_text(
        f"Package: screennotes\nVersion: {version}\nArchitecture: {arch}\n"
        f"Maintainer: {args.maintainer}\nSection: graphics\nPriority: optional\n"
        f"Installed-Size: {size}\nDepends: {dependencies}\n"
        "Recommends: xdg-desktop-portal-gnome | xdg-desktop-portal-kde, gnome-shell-extension-appindicator\n"
        "Description: Capture and annotate screenshots on Wayland\n"
        " Select a region across monitors and add text, lines, and rectangles\n"
        " directly over the captured desktop. Copy to the clipboard or save PNG.\n"
    )
    artifact = args.output_dir / f"screennotes_{version}_{arch}.deb"
    run("dpkg-deb", "--root-owner-group", "--build", directory, artifact)
    run("dpkg-deb", "--info", artifact)
    record(artifact, {"version": version, "architecture": arch, "depends": dependencies})


def fetch_tool(tool, destination):
    if not destination.exists() or checksum(destination) != tool["sha256"]:
        require("curl")
        temporary = destination.with_suffix(destination.suffix + ".download")
        run("curl", "--fail", "--location", "--retry", "3", tool["url"], "--output", temporary)
        if checksum(temporary) != tool["sha256"]:
            raise RuntimeError(f"Checksum mismatch for {destination.name}. Review and update packaging/tools.json; no downloaded code was executed.")
        temporary.replace(destination)
    destination.chmod(0o755)
    return destination


def appimage(binary, version, arch, args, work):
    require("pkg-config")
    # Modern Pixbuf uses separate Glycin loader processes. The plugin does not
    # bundle those yet; a 24.04 build avoids generating a silently broken image.
    if run("pkg-config", "--atleast-version=2.44", "gdk-pixbuf-2.0", check=False).returncode == 0:
        raise RuntimeError("AppImage builds require GDK Pixbuf < 2.44. Use scripts/package-container.sh appimage (Ubuntu 24.04).")
    run("pkg-config", "--atleast-version=4.10", "gtk4")
    app_arch = {"amd64": "x86_64", "arm64": "aarch64"}[arch]
    tools = json.loads((ROOT / "packaging/tools.json").read_text())
    cache = args.work_dir / "tools"
    cache.mkdir(parents=True, exist_ok=True)
    deploy = args.linuxdeploy or fetch_tool(tools[app_arch], cache / f"linuxdeploy-{app_arch}.AppImage")
    plugin = args.gtk_plugin or fetch_tool(tools["gtk"], cache / "linuxdeploy-plugin-gtk.sh")
    # Extract the build tool: no FUSE device or privileged mount is needed.
    extracted = work / "linuxdeploy"
    extracted.mkdir()
    run(deploy, "--appimage-extract", cwd=extracted, stdout=subprocess.DEVNULL)
    deploy = extracted / "squashfs-root/AppRun"
    install(plugin, extracted / "squashfs-root/usr/bin/linuxdeploy-plugin-gtk.sh", 0o755)
    directory = work / "ScreenNotes.AppDir"
    stage(binary, directory)
    env = {**os.environ, "DEPLOY_GTK_VERSION": "4", "APPIMAGE_EXTRACT_AND_RUN": "1", "NO_STRIP": "1"}
    run(deploy, "--appdir", directory, "--executable", directory / "usr/bin/screennotes",
        "--desktop-file", directory / f"usr/share/applications/{APP_ID}.desktop",
        "--icon-file", directory / "usr/share/icons/hicolor/256x256/apps/screennotes.png",
        "--plugin", "gtk", env=env)
    hook = directory / "apprun-hooks/linuxdeploy-plugin-gtk.sh"
    content = hook.read_text()
    content, count = re.subn(r"^export GDK_BACKEND=x11[^\n]*$", '# Preserve the session backend and any explicit user override.', content, flags=re.MULTILINE)
    if count != 1:
        raise RuntimeError("GTK plugin backend hook changed. Review it before building a Wayland AppImage.")
    hook.write_text(content)
    # linuxdeploy prefers the installed SVG, but appimagetool requires a raster
    # root icon for thumbnail/desktop integration. Provide both explicitly.
    install(ROOT / "assets/previews/screennotes-256.png", directory / "screennotes.png")
    install(ROOT / "assets/previews/screennotes-256.png", directory / ".DirIcon")
    # Include redistribution notices for bundled libraries from the build system.
    notices = directory / "usr/share/doc/screennotes/bundled-libraries"
    notices.mkdir(parents=True)
    for copyright_file in Path("/usr/share/doc").glob("*/copyright"):
        install(copyright_file, notices / (copyright_file.parent.name + ".copyright"))
    artifact = args.output_dir / f"ScreenNotes-{version}-{app_arch}.AppImage"
    env.update({"LDAI_OUTPUT": str(artifact), "LDAI_VERSION": version, "LDAI_NO_APPSTREAM": "1", "ARCH": app_arch})
    run(deploy, "--appdir", directory, "--output", "appimage", env=env)
    if not artifact.is_file():
        raise RuntimeError("The AppImage output plugin did not create the requested artifact.")
    artifact.chmod(0o755)
    record(artifact, {"version": version, "architecture": app_arch, "toolchain": tools})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("format", choices=["deb", "appimage", "all"])
    parser.add_argument("--skip-build", action="store_true", help="Package an existing release binary")
    parser.add_argument("--binary", type=Path, help="Existing native binary; requires --skip-build")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/packaging")
    parser.add_argument("--maintainer", default="ScreenNotes contributors", help="Debian maintainer, ideally 'Name <email>'")
    parser.add_argument("--linuxdeploy", type=Path, help="Use a local linuxdeploy AppImage instead of downloading")
    parser.add_argument("--gtk-plugin", type=Path, help="Use a local linuxdeploy GTK plugin instead of downloading")
    args = parser.parse_args()
    if args.binary and not args.skip_build:
        parser.error("--binary requires --skip-build")
    if any(c in args.maintainer for c in "\r\n"):
        parser.error("--maintainer must be a single line")
    require("readelf", "dpkg", "desktop-file-validate")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    if not re.fullmatch(r"[0-9][A-Za-z0-9.+~:-]*", version):
        raise RuntimeError("Cargo package version is not a valid Debian version.")
    arch = output("dpkg", "--print-architecture")
    if arch not in ("amd64", "arm64"):
        raise RuntimeError("Only native amd64 and arm64 builds are supported.")
    if not args.skip_build:
        require("cargo", "pkg-config")
        run("pkg-config", "--atleast-version=4.10", "gtk4")
        run("cargo", "build", "--release", "--locked", cwd=ROOT)
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = (args.binary or target / "release/screennotes").resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError(f"Release binary not found: {binary}")
    header = output("readelf", "-h", binary, env={**os.environ, "LC_ALL": "C"})
    if {"amd64": "Advanced Micro Devices X86-64", "arm64": "AArch64"}[arch] not in header:
        raise RuntimeError("Binary architecture does not match the build host. Cross-packaging is unsupported.")
    for name in ("output_dir", "work_dir", "linuxdeploy", "gtk_plugin"):
        if value := getattr(args, name):
            setattr(args, name, value.resolve())
    args.output_dir.mkdir(parents=True, exist_ok=True)
    args.work_dir.mkdir(parents=True, exist_ok=True)
    # Only this newly created staging directory is removed, never user paths.
    with tempfile.TemporaryDirectory(prefix="build-", dir=args.work_dir) as temporary:
        work = Path(temporary)
        if args.format in ("deb", "all"):
            deb(binary, version, arch, args, work)
        if args.format in ("appimage", "all"):
            appimage(binary, version, arch, args, work)


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        sys.exit(1)
