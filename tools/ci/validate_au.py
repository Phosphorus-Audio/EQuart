"""Validate the packaged AU without installing its separate CLAP bundle."""

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "packaging"))
from release import validate_audio_unit


def main():
    if sys.platform != "darwin":
        raise SystemExit("Native Audio Unit validation requires macOS")

    archives = list((ROOT / "dist").glob("*-macos-universal.zip"))
    if len(archives) != 1:
        raise ValueError(f"Expected one macOS archive, found {len(archives)}")

    with tempfile.TemporaryDirectory(prefix="equart-au-check-") as work:
        stage = Path(work) / "stage"
        subprocess.run(["ditto", "-x", "-k", str(archives[0]), str(stage)], check=True)
        component = stage / "equart.component"
        validate_audio_unit(component)
        destination = Path("/Library/Audio/Plug-Ins/Components/equart.component")
        subprocess.run(["sudo", "ditto", str(component), str(destination)], check=True)

    subprocess.run(["codesign", "--verify", "--deep", "--strict", str(destination)], check=True)
    inspector = ROOT / "target/au-inspect"
    subprocess.run(["xcrun", "clang++", "-std=c++17", str(ROOT / "tools/ci/inspect_au.cpp"),
                    "-framework", "CoreFoundation", "-o", str(inspector)], check=True)
    subprocess.run([str(inspector), str(destination)], check=True)
    registrar = subprocess.run(["pgrep", "-x", "AudioComponentRegistrar"], capture_output=True)
    if registrar.returncode == 0:
        subprocess.run(["sudo", "killall", "-9", "AudioComponentRegistrar"], check=True)
    elif registrar.returncode != 1:
        registrar.check_returncode()

    subprocess.run(["auval", "-a"], check=True)
    subprocess.run(["auval", "-strict", "-v", "aufx", "Eqrt", "PhAu"], check=True)


if __name__ == "__main__":
    main()
