#!/usr/bin/env python3
"""Update the application version without changing dependencies or publishing."""
import argparse
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def validate(version):
    match = SEMVER.fullmatch(version)
    if not match or any(
        part.isdigit() and len(part) > 1 and part.startswith("0")
        for part in (match[4] or "").split(".")
    ):
        raise ValueError(f"Invalid semantic version: {version}")
    return match


def replace_version(block, version):
    result, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"\n]+"',
        lambda match: f'{match[1]}"{version}"', block,
    )
    if count != 1:
        raise ValueError("Expected exactly one version field in the package block")
    return result


def updates(root, requested):
    manifest_path = root / "Cargo.toml"
    lock_path = root / "Cargo.lock"
    manifest = manifest_path.read_text()
    lock = lock_path.read_text()
    package = tomllib.loads(manifest)["package"]
    old = package["version"]
    parts = validate(old)
    if requested in ("major", "minor", "patch"):
        numbers = [int(parts[i]) for i in (1, 2, 3)]
        index = ("major", "minor", "patch").index(requested)
        numbers[index] += 1
        numbers[index + 1:] = [0] * (2 - index)
        new = ".".join(map(str, numbers))
    else:
        new = requested.removeprefix("v")
    validate(new)
    entries = [p for p in tomllib.loads(lock)["package"]
               if p["name"] == package["name"] and "source" not in p]
    if len(entries) != 1 or entries[0]["version"] != old:
        raise ValueError("Cargo.lock must contain one matching local package with the current version")
    block = re.search(r'(?ms)^\[package\]\s*\n.*?(?=^\[|\Z)', manifest)
    if not block:
        raise ValueError("Could not locate [package] in Cargo.toml")
    manifest = manifest[:block.start()] + replace_version(block[0], new) + manifest[block.end():]
    blocks = re.split(r'(?m)(?=^\[\[package\]\])', lock)
    for index, block in enumerate(blocks):
        if block.startswith("[[package]]"):
            entry = tomllib.loads(block)["package"][0]
            if entry["name"] == package["name"] and "source" not in entry:
                blocks[index] = replace_version(block, new)
    changes = {manifest_path: manifest, lock_path: "".join(blocks)}
    # Update download/tag examples, preserving historical version references.
    for path in (root / "README.md", root / "packaging/README.md"):
        text = path.read_text()
        for prefix in ("ScreenNotes-", "screennotes_", "git tag v", "git push origin v", "ScreenNotes v"):
            text = text.replace(prefix + old, prefix + new)
        text = text.replace("\nv" + old + "**", "\nv" + new + "**")
        changes[path] = text
    for path in (manifest_path, lock_path):
        tomllib.loads(changes[path])
    return old, new, {path: text for path, text in changes.items() if path.read_text() != text}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="A version (e.g. 0.2.0 or v0.2.0-beta.1), or patch/minor/major")
    parser.add_argument("--dry-run", action="store_true", help="Show changes without writing files")
    args = parser.parse_args()
    old, new, changes = updates(ROOT, args.version)
    print(f"ScreenNotes: {old} → {new}")
    if not changes:
        print("Already up to date.")
        return
    for path, text in changes.items():
        if not args.dry_run:
            path.write_text(text)
        print(f"{'Would update' if args.dry_run else 'Updated'} {path.relative_to(ROOT)}")
    print(f"Release tag: v{new}. No commit, tag, build or push was performed.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        print(f"Version update failed: {error}", file=sys.stderr)
        sys.exit(1)
