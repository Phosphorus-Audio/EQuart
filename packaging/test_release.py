"""Portable checks for the macOS Audio Unit packaging contract."""

import plistlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release


class AudioUnitPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "equart.component"
        self.metadata = {
            "AudioComponents": [{
                "type": "aufx", "subtype": "Eqrt", "manufacturer": "PhAu",
                "name": "Phosphorus Audio: EQuart", "factoryFunction": "factory",
            }],
        }
        for path in (self.bundle / "Contents/MacOS/equart",
                     self.bundle / "Contents/PlugIns/equart.clap/Contents/MacOS/equart"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"binary")
        self.write_metadata()

    def write_metadata(self):
        with (self.bundle / "Contents/Info.plist").open("wb") as output:
            plistlib.dump(self.metadata, output)

    def test_requires_embedded_engine_and_correct_identity(self):
        release.validate_audio_unit(self.bundle)
        embedded = self.bundle / "Contents/PlugIns/equart.clap/Contents/MacOS/equart"
        embedded.unlink()
        with self.assertRaises(FileNotFoundError):
            release.validate_audio_unit(self.bundle)
        embedded.write_bytes(b"binary")
        self.metadata["AudioComponents"][0]["subtype"] = "wrong"
        self.write_metadata()
        with self.assertRaises(ValueError):
            release.validate_audio_unit(self.bundle)

    def test_stage_includes_au_only_on_macos(self):
        with patch.object(release, "ROOT", self.root):
            bundled = self.root / "target/bundled"
            bundled.mkdir(parents=True)
            for name in release.FORMATS:
                (bundled / name).write_bytes(b"bundle")
            (bundled / "equart.component").mkdir()
            for name in ("README.md", "LICENSE"):
                (self.root / name).write_text("license", encoding="utf-8")
            fonts = self.root / "assets/fonts"
            fonts.mkdir(parents=True)
            (fonts / "UFL.txt").write_text("license", encoding="utf-8")
            au_licenses = bundled / "au-licenses"
            au_licenses.mkdir()
            for name in ("clap-wrapper-MIT.txt", "clap-MIT.txt", "AudioUnitSDK.txt"):
                (au_licenses / name).write_text("license", encoding="utf-8")

            for platform in ("macos", "windows"):
                stage = self.root / platform
                stage.mkdir()
                release.stage_bundles(stage, platform)
                self.assertEqual((stage / "equart.component").exists(), platform == "macos")
                self.assertEqual((stage / "licenses/AudioUnitSDK.txt").exists(), platform == "macos")


if __name__ == "__main__":
    unittest.main()
