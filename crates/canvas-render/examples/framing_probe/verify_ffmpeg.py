"""Validaci?n real: requiere Python con Pillow, FFmpeg y CLI de Flashcut-Auto.

Primero: cargo run -p canvas-render --example framing_probe -- target/framing-probe
Luego: python <este archivo> target/framing-probe <ruta-a-cli.exe>
Todas las salidas van a una subcarpeta nueva del fixture.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from PIL import Image, ImageChops, ImageStat


def run(args):
    result = subprocess.run([str(arg) for arg in args], capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr)


def geometry(fixture, output):
    source = output / "geometry-source.png"
    row_colors = [(round(x / 1919 * 255), 128) for x in range(1920)]
    raw = bytearray()
    for y in range(1080):
        for red, blue in row_colors:
            raw.extend((red, round(y / 1079 * 255), blue))
    Image.frombytes("RGB", (1920, 1080), bytes(raw)).save(source)
    with (fixture / "geometry.csv").open(encoding="utf-8") as csv_file:
        cases = list(csv.DictReader(csv_file))
    assert len(cases) == 27
    for row in cases:
        x, y, scale = [float(row[key]) for key in ("x", "y", "scale")]
        left, top, width, height = [float(row[key]) for key in ("left", "top", "width", "height")]
        zoom = "" if scale == 100 else f",scale=trunc(iw*{scale/100}/2)*2:trunc(ih*{scale/100}/2)*2"
        # Fondo negro de m?scara para medir l?mites; no compara color YUV.
        chain = (
            "color=black:s=1080x1920[bg];[0:v]"
            f"scale=1080:1920:force_original_aspect_ratio=increase{zoom},setsar=1[fg];"
            "[bg][fg]overlay="
            f"x='if(gte(W-w,0),clip((W-w)/2+({x}/100)*W,0,W-w),clip((W-w)/2-({x}/100)*W,W-w,0))':"
            f"y='if(gte(H-h,0),clip((H-h)/2+({y}/100)*H,0,H-h),clip((H-h)/2-({y}/100)*H,H-h,0))':format=rgb"
        )
        frame = output / "geometry-check.png"
        run(["ffmpeg", "-v", "error", "-y", "-i", source, "-filter_complex", chain, "-frames:v", "1", frame])
        image = Image.open(frame).convert("RGB")
        bounds = image.getchannel("B").point(lambda value: 255 if value > 60 else 0).getbbox()
        expected = (max(0, left), max(0, top), min(1080, left + width), min(1920, top + height))
        assert max(abs(a - b) for a, b in zip(bounds, expected)) <= 2, (row, bounds, expected)
        for fraction in (0.25, 0.5, 0.75):
            px = round(expected[0] + (expected[2] - expected[0]) * fraction)
            py = round(expected[1] + (expected[3] - expected[1]) * fraction)
            red, green, _ = image.getpixel((px, py))
            assert abs(red - (px - left) / width * 255) <= 3, row
            assert abs(green - (py - top) / height * 255) <= 3, row
    print("FFMPEG_GEOMETRY=27 cases passed; bounds <=2px and source-coordinate samples")


def flashcut(fixture, output, cli):
    plan = {
        "schemaVersion": 2,
        "project": {"title": "Framing QA", "baseDir": str(fixture), "fps": 30, "width": 1080, "height": 1920},
        "timeline": {
            "tracks": [{"id": "v1", "clips": [{"id": "c1", "asset": "composition.png", "durationS": 1.0}]}],
            "transitions": [], "captions": {"styleVersion": "v1"},
            "audio": {"gainDb": 0.0, "ducking": False}, "color": {"lutVersion": "v1"},
        },
        "renderer": {"kind": "ffmpeg-direct"},
    }
    assets = [fixture / "composition.canvas", fixture / "composition.png"]
    before = [hashlib.sha256(path.read_bytes()).digest() for path in assets]
    plan_path = output / "plan.json"
    framed_path = output / "framed-plan.json"
    plan_path.write_text(json.dumps(plan), encoding="utf-8")
    run([cli, "framing", "apply", "--plan", plan_path, "--output", framed_path])
    assert json.loads(framed_path.read_text(encoding="utf-8"))["timeline"]["tracks"][0]["clips"][0]["framing"]["scalePct"] == 50
    video = output / "auto-short.mp4"
    run([cli, "render", framed_path, "--output", video, "--timeout", "60"])
    run(["ffmpeg", "-v", "error", "-i", video, "-frames:v", "1", output / "auto-frame.png"])
    # Un plan expl?cito debe conservar su framing aunque el sidecar tenga otro.
    explicit = {"xPct": 20.0, "yPct": -10.0, "scalePct": 75}
    plan["timeline"]["tracks"][0]["clips"][0]["framing"] = explicit
    plan_path.write_text(json.dumps(plan), encoding="utf-8")
    explicit_path = output / "explicit-plan.json"
    run([cli, "framing", "apply", "--plan", plan_path, "--output", explicit_path])
    assert json.loads(explicit_path.read_text(encoding="utf-8"))["timeline"]["tracks"][0]["clips"][0]["framing"] == explicit
    assert [hashlib.sha256(path.read_bytes()).digest() for path in assets] == before
    print("FLASHCUT_AUTO=real 1080x1920 MP4; explicit plan preserved; PNG/canvas hashes unchanged")


def comparison(fixture, output):
    reference = Image.open(fixture / "canvas-preview-reference.png").convert("RGB")
    actual = Image.open(output / "auto-frame.png").convert("RGB")
    assert reference.size == actual.size == (1080, 1920)
    region = (400, 880, 700, 1140)
    def red_bounds(image):
        patch = image.crop(region)
        red, green, blue = patch.split()
        mask = ImageChops.multiply(red.point(lambda value: 255 if value > 200 else 0), green.point(lambda value: 255 if value < 100 else 0))
        mask = ImageChops.multiply(mask, blue.point(lambda value: 255 if value < 130 else 0))
        return mask.getbbox()
    assert max(abs(a - b) for a, b in zip(red_bounds(reference), red_bounds(actual))) <= 2
    mean = ImageStat.Stat(ImageChops.difference(reference, actual)).mean
    print("PREVIEW_COMPARISON=foreground bounds <=2px; mean absolute RGB difference", tuple(round(value, 2) for value in mean))
    print("Reduced blur, scaling and MP4 color conversion differ; no pixel-identical blur claim.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fixture", type=Path)
    parser.add_argument("cli", type=Path)
    args = parser.parse_args()
    fixture = args.fixture.resolve()
    output = Path(tempfile.mkdtemp(prefix="framing-validation-", dir=fixture))
    geometry(fixture, output)
    flashcut(fixture, output, args.cli.resolve())
    comparison(fixture, output)
    print("VALIDATION_ARTIFACTS=" + str(output))


if __name__ == "__main__":
    main()
