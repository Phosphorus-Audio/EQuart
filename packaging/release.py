"""Package the EQuart plugin bundles for the current CI runner."""

import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile


ROOT = Path(__file__).resolve().parents[1]
FORMATS = {"equart.clap": "CLAP", "equart.vst3": "VST3"}
MACOS_MIN = "14.0"


def run(*args):
    subprocess.run(args, check=True)


def stage_bundles(stage, platform):
    formats = dict(FORMATS)
    if platform == "macos":
        formats["equart.component"] = "Components"

    for name in formats:
        source = ROOT / "target" / "bundled" / name
        if source.is_dir():
            shutil.copytree(source, stage / name, symlinks=True)
        else:
            shutil.copy2(source, stage / name)

    for name in ("README.md", "LICENSE"):
        shutil.copy2(ROOT / name, stage)

    licenses = stage / "licenses"
    for directory in (
        "assets/fonts",
        "assets/icons",
        "vendor/baseview",
        "vendor/egui-baseview",
        "vendor/nice-plug",
        "vendor/pitch-estimate",
    ):
        sources = ROOT / directory
        target = licenses / sources.name
        target.mkdir(parents=True)
        for source in sorted(sources.glob("LICENSE*")):
            shutil.copy2(source, target)
        if directory == "assets/fonts":
            shutil.copy2(sources / "UFL.txt", target)

    if platform == "macos":
        for name in ("clap-wrapper-MIT.txt", "clap-MIT.txt", "AudioUnitSDK.txt"):
            shutil.copy2(ROOT / "target/bundled/au-licenses" / name, licenses)


def validate_audio_unit(bundle):
    with (bundle / "Contents/Info.plist").open("rb") as source:
        metadata = plistlib.load(source)
    components = metadata["AudioComponents"]
    identities = [(c["type"], c["subtype"], c["manufacturer"]) for c in components]
    if identities != [("aufx", "Eqrt", "PhAu")]:
        raise ValueError(f"Unexpected Audio Unit identity: {identities}")
    if not components[0]["name"] or not components[0]["factoryFunction"]:
        raise ValueError("Audio Unit name or factory entry point is missing")
    for path in (bundle / "Contents/MacOS/equart",
                 bundle / "Contents/PlugIns/equart.clap/Contents/MacOS/equart"):
        if not path.is_file():
            raise FileNotFoundError(path)


def validate_binary(path, platform):
    with path.open("rb") as binary:
        header = binary.read(64)
        if platform == "windows" and header[:2] == b"MZ":
            binary.seek(int.from_bytes(header[60:64], "little"))
            if binary.read(6) == b"PE\0\0\x64\x86":
                return
        if platform == "linux" and header[:6] == b"\x7fELF\x02\x01" and header[18:20] == b"\x3e\0":
            return
    raise ValueError(f"Expected an x86-64 {platform} binary: {path}")


def zip_stage(stage, destination):
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(stage.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(stage))
    with zipfile.ZipFile(destination) as archive:
        bad_entry = archive.testzip()
        if bad_entry is not None:
            raise ValueError(f"Corrupt archive entry: {bad_entry}")


def package_macos(stage, stem, version):
    formats = {**FORMATS, "equart.component": "Components"}
    for name, directory in formats.items():
        bundle = stage / name
        if name == "equart.component":
            validate_audio_unit(bundle)
            embedded = bundle / "Contents/PlugIns/equart.clap"
            shutil.copytree(stage / "equart.clap", embedded, symlinks=True, dirs_exist_ok=True)
        binary = bundle / "Contents/MacOS/equart"
        plist = bundle / "Contents/Info.plist"
        with plist.open("rb") as source:
            metadata = plistlib.load(source)
        metadata.update(
            CFBundleIdentifier=f"audio.equart.eq.{name.rsplit('.', 1)[1]}",
            CFBundleShortVersionString=version,
            CFBundleVersion=version,
            LSMinimumSystemVersion=MACOS_MIN,
        )
        with plist.open("wb") as destination:
            plistlib.dump(metadata, destination)

        run("lipo", binary, "-verify_arch", "x86_64", "arm64")
        run("codesign", "--force", "--sign", "-", "--timestamp=none", bundle)
        run("codesign", "--verify", "--deep", "--strict", bundle)

    payload = stage.parent / "payload"
    for name, directory in formats.items():
        shutil.copytree(stage / name, payload / "Library/Audio/Plug-Ins" / directory / name, symlinks=True)

    support = payload / "Library/Application Support/EQuart"
    shutil.copytree(stage / "licenses", support / "licenses")
    for name in ("README.md", "LICENSE"):
        shutil.copy2(stage / name, support)

    components = stage.parent / "components.plist"
    run("pkgbuild", "--analyze", "--root", payload, components)
    with components.open("rb") as source:
        bundles = plistlib.load(source)
    for bundle in bundles:
        bundle["BundleIsRelocatable"] = False
    with components.open("wb") as destination:
        plistlib.dump(bundles, destination)

    pkg = stem.parent / f"{stem.name}.pkg"
    run(
        "pkgbuild", "--root", payload, "--identifier", "audio.equart.eq",
        "--version", version, "--install-location", "/", "--component-plist",
        components, "--ownership", "recommended", pkg,
    )
    run("ditto", "-c", "-k", "--sequesterRsrc", stage, stem.parent / f"{stem.name}.zip")


def main(platform):
    expected = {"windows": "win32", "macos": "darwin", "linux": "linux"}
    if sys.platform != expected[platform]:
        raise ValueError(f"Packaging {platform} requires {expected[platform]}, got {sys.platform}")

    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    tag = os.environ.get("GITHUB_REF_NAME")
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and tag != f"v{version}":
        raise ValueError(f"Tag {tag} must match Cargo.toml version v{version}")

    output = ROOT / "dist"
    output.mkdir(exist_ok=True)
    arch = "universal" if platform == "macos" else "x64"
    stem = output / f"ppa-equart-{version}-{platform}-{arch}"

    with tempfile.TemporaryDirectory(prefix="equart-release-") as work:
        stage = Path(work) / "stage"
        stage.mkdir()
        stage_bundles(stage, platform)
        if platform == "macos":
            package_macos(stage, stem, version)
            return

        vst3 = {
            "windows": "x86_64-win/equart.vst3",
            "linux": "x86_64-linux/equart.so",
        }[platform]
        validate_binary(stage / "equart.clap", platform)
        validate_binary(stage / "equart.vst3/Contents" / vst3, platform)

        if platform == "windows":
            run(
                os.environ["ISCC"], f"/DAppVersion={version}", f"/DSourceDir={stage}",
                f"/DOutputDir={output}", ROOT / "packaging/windows/equart.iss",
            )
            zip_stage(stage, output / f"{stem.name}.zip")
        else:
            with tarfile.open(output / f"{stem.name}.tar.gz", "w:gz") as archive:
                archive.add(stage, arcname=stem.name)


if __name__ == "__main__":
    main(sys.argv[1])
