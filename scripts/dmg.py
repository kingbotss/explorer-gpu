#!/usr/bin/env python3
"""Package the existing signed app without rebuilding or mounting disk images."""
from pathlib import Path
import os
import plistlib
import subprocess
import tempfile


def run(*arguments):
    subprocess.run(arguments, check=True)


def main():
    project = Path(__file__).resolve().parent.parent
    distribution = project / "dist"
    app = distribution / "Explorer.app"
    if not app.is_dir():
        raise SystemExit("Build the app with scripts/bundle.sh first.")
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    version = info["CFBundleShortVersionString"]
    output = distribution / f"Explorer-{version}-macOS.dmg"
    run("codesign", "--verify", "--deep", "--strict", str(app))

    with tempfile.TemporaryDirectory(prefix=".explorer-dmg-", dir=distribution) as temporary:
        work = Path(temporary)
        stage = work / "Explorer"
        stage.mkdir()
        run("ditto", str(app), str(stage / "Explorer.app"))
        (stage / "Applications").symlink_to("/Applications")
        (stage / "Read Me.txt").write_text(
            "Install Explorer\n\n"
            "1. Quit any running copy of Explorer.\n"
            "2. Drag Explorer.app onto Applications.\n"
            "3. Open Explorer from Applications.\n\n"
            "The installed app runs independently of the source project and its build cache.\n"
        )
        run("codesign", "--verify", "--deep", "--strict", str(stage / "Explorer.app"))
        filesystem = work / "Explorer-filesystem.dmg"
        compressed = work / "Explorer-compressed.dmg"
        # makehybrid writes HFS+ directly, avoiding a temporary mounted device.
        run(
            "hdiutil", "makehybrid", "-hfs", "-hfs-volume-name", "Explorer",
            "-hfs-openfolder", str(stage), "-o", str(filesystem), str(stage),
        )
        run("hdiutil", "convert", str(filesystem), "-format", "UDZO", "-o", str(compressed))
        run("hdiutil", "verify", str(compressed))
        os.replace(compressed, output)
    print(f"Created {output}")


if __name__ == "__main__":
    main()
