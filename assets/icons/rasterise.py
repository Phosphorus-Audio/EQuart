"""Flatten the SVG paths of the editor icons and rasterise them to alpha masks.

The editor has no SVG runtime, so the icons are baked: this script converts the
paths to coverage masks, which are checked into `assets/icons/*.alpha` and
uploaded once as textures. Nonzero winding, 4x supersampling, box downsampled.
"""

import math
import re
from pathlib import Path
from xml.etree import ElementTree

import numpy as np
from PIL import Image

SS = 4  # sub-samples per axis


def tokenize(d):
    pattern = re.compile(r"([MmLlHhVvCcAaZz])|(-?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)")
    out = []
    for command, number in pattern.findall(d):
        out.append(command if command else float(number))
    return out


def cubic(p0, p1, p2, p3, steps):
    for i in range(1, steps + 1):
        t = i / steps
        u = 1.0 - t
        yield (
            u * u * u * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0],
            u * u * u * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1],
        )


def arc(p0, rx, ry, rotation, large, sweep, p1, steps=24):
    """Endpoint parameterisation, as in the SVG spec, flattened by angle."""
    if rx == 0.0 or ry == 0.0:
        yield p1
        return
    rx, ry = abs(rx), abs(ry)
    phi = math.radians(rotation)
    cos_phi, sin_phi = math.cos(phi), math.sin(phi)
    dx2 = (p0[0] - p1[0]) / 2.0
    dy2 = (p0[1] - p1[1]) / 2.0
    x1p = cos_phi * dx2 + sin_phi * dy2
    y1p = -sin_phi * dx2 + cos_phi * dy2
    scale = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
    if scale > 1.0:
        rx *= math.sqrt(scale)
        ry *= math.sqrt(scale)
    numerator = max(0.0, rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p)
    denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    factor = math.sqrt(numerator / denominator) if denominator else 0.0
    if large == sweep:
        factor = -factor
    cxp = factor * rx * y1p / ry
    cyp = -factor * ry * x1p / rx
    cx = cos_phi * cxp - sin_phi * cyp + (p0[0] + p1[0]) / 2.0
    cy = sin_phi * cxp + cos_phi * cyp + (p0[1] + p1[1]) / 2.0

    def angle(ux, uy, vx, vy):
        dot = ux * vx + uy * vy
        length = math.hypot(ux, uy) * math.hypot(vx, vy)
        value = max(-1.0, min(1.0, dot / length)) if length else 1.0
        sign = 1.0 if ux * vy - uy * vx >= 0 else -1.0
        return sign * math.acos(value)

    ux = (x1p - cxp) / rx
    uy = (y1p - cyp) / ry
    vx = (-x1p - cxp) / rx
    vy = (-y1p - cyp) / ry
    start = angle(1.0, 0.0, ux, uy)
    sweep_angle = angle(ux, uy, vx, vy)
    if sweep == 0 and sweep_angle > 0:
        sweep_angle -= 2 * math.pi
    elif sweep == 1 and sweep_angle < 0:
        sweep_angle += 2 * math.pi

    count = max(6, int(abs(sweep_angle) / (math.pi / 32)) + 1)
    for i in range(1, count + 1):
        t = start + sweep_angle * i / count
        x = rx * math.cos(t)
        y = ry * math.sin(t)
        yield (cos_phi * x - sin_phi * y + cx, sin_phi * x + cos_phi * y + cy)


def subpaths(d):
    tokens = tokenize(d)
    i = 0
    current = (0.0, 0.0)
    start = (0.0, 0.0)
    command = None
    points = []
    out = []

    def flush():
        if len(points) > 2:
            out.append(list(points))
        points.clear()

    while i < len(tokens):
        if isinstance(tokens[i], str):
            command = tokens[i]
            i += 1
            if command in "Zz":
                flush()
                current = start
                continue
        if command is None:
            break

        def take(n):
            nonlocal i
            values = tokens[i : i + n]
            i += n
            return [float(v) for v in values]

        relative = command.islower()
        letter = command.upper()

        if letter == "M":
            x, y = take(2)
            if relative:
                x, y = current[0] + x, current[1] + y
            flush()
            current = (x, y)
            start = current
            points.append(current)
            command = "l" if relative else "L"
        elif letter == "L":
            x, y = take(2)
            if relative:
                x, y = current[0] + x, current[1] + y
            points.append((x, y))
            current = (x, y)
        elif letter == "H":
            (x,) = take(1)
            x = current[0] + x if relative else x
            points.append((x, current[1]))
            current = (x, current[1])
        elif letter == "V":
            (y,) = take(1)
            y = current[1] + y if relative else y
            points.append((current[0], y))
            current = (current[0], y)
        elif letter == "C":
            x1, y1, x2, y2, x, y = take(6)
            if relative:
                x1, y1 = current[0] + x1, current[1] + y1
                x2, y2 = current[0] + x2, current[1] + y2
                x, y = current[0] + x, current[1] + y
            length = (
                math.dist(current, (x1, y1))
                + math.dist((x1, y1), (x2, y2))
                + math.dist((x2, y2), (x, y))
            )
            steps = max(4, int(length / 3.0))
            points.extend(cubic(current, (x1, y1), (x2, y2), (x, y), steps))
            current = (x, y)
        elif letter == "A":
            rx, ry, rotation, large, sweep, x, y = take(7)
            if relative:
                x, y = current[0] + x, current[1] + y
            points.extend(arc(current, rx, ry, rotation, int(large), int(sweep), (x, y)))
            current = (x, y)
        else:
            raise ValueError(f"unsupported command {command}")

    flush()
    return out


def rasterise(paths, size=256, supersample=SS, even_odd=False):
    """Scanline fill with exact horizontal coverage, nonzero or even-odd."""
    height = size * supersample
    rows = np.zeros((height, size * supersample), dtype=np.float32)

    edges = []
    for sub in paths:
        for a, b in zip(sub, sub[1:] + sub[:1]):
            if a[1] == b[1]:
                continue
            edges.append((a, b))

    for row in range(height):
        y = (row + 0.5) / supersample
        crossings = []
        for a, b in edges:
            if (a[1] <= y < b[1]) or (b[1] <= y < a[1]):
                t = (y - a[1]) / (b[1] - a[1])
                x = a[0] + t * (b[0] - a[0])
                crossings.append((x, 1 if b[1] > a[1] else -1))
        if not crossings:
            continue
        crossings.sort()
        winding = 0
        for index in range(len(crossings) - 1):
            winding += crossings[index][1]
            inside = (winding % 2 != 0) if even_odd else (winding != 0)
            if not inside:
                continue
            x0 = crossings[index][0] * supersample
            x1 = crossings[index + 1][0] * supersample
            col0 = max(0, int(math.floor(x0)))
            col1 = min(rows.shape[1], int(math.ceil(x1)))
            for col in range(col0, col1):
                coverage = min(x1, col + 1) - max(x0, col)
                if coverage > 0:
                    rows[row, col] += coverage

    # Box filter the supersampled coverage down to one value per output pixel.
    small = rows.reshape(size, supersample, size, supersample).mean(axis=(1, 3))
    return np.clip(small, 0.0, 1.0)


def main():
    icons = {
        "bypass": "M168.49,151.51a12,12,0,0,1-17,17l-64-64a12,12,0,0,1,17-17ZM236,128A108,108,0,1,1,128,20,108.12,108.12,0,0,1,236,128Zm-24,0a84,84,0,1,0-84,84A84.09,84.09,0,0,0,212,128Z",
        # The supplied glyph is a solid tile with two corners notched out; at
        # 18 px that reads as a filled square, and its 16-unit frame is barely
        # over a pixel. Four corner brackets carry the same "resize" reading
        # and stay legible at button size.
        "scale": "M24,86V24H86V46H46V86Z"
        " M170,24H232V86H210V46H170Z"
        " M24,170V232H86V210H46V170Z"
        " M232,170V232H170V210H210V170Z",
        "options": "M232,96a72,72,0,0,1-100.94,66L79,222.22c-.12.14-.26.29-.39.42a32,32,0,0,1-45.26-45.26c.14-.13.28-.27.43-.39L94,124.94a72.07,72.07,0,0,1,83.54-98.78,8,8,0,0,1,3.93,13.19L144,80l5.66,26.35L176,112l40.65-37.52a8,8,0,0,1,13.19,3.93A72.6,72.6,0,0,1,232,96Z",
    }
    for name in (
        "x-bold",
        "trash-simple-bold",
        "plus-bold",
        "dots-six-bold",
        "align-center-horizontal-simple-bold",
        "circle-bold",
        "layout-bold",
        "headphones-bold",
    ):
        source = Path("assets/icons", f"{name}.svg").read_text(encoding="utf-8")
        path = ElementTree.fromstring(source).find("{http://www.w3.org/2000/svg}path")
        icons[name] = path.attrib["d"]

    # The paths are drawn in Phosphor's 256-unit box, so rasterise at 1:1 and
    # box-filter down to the stored size.
    out_size = 32
    size = 256
    factor = size // out_size
    for name, path in icons.items():
        raw = rasterise(
            subpaths(path), size=size, supersample=SS, even_odd=(name == "scale")
        )
        # `rasterise` already box-filtered the supersamples, so this is a plain
        # area average over the 8x8 blocks that make up one stored pixel.
        mask = raw.reshape(out_size, factor, out_size, factor).mean(axis=(1, 3))
        alpha = np.round(mask * 255.0).astype(np.uint8)
        alpha.tofile(f"assets/icons/{name}.alpha")
        Image.fromarray(alpha, "L").save(f"target/{name}_icon.png")
        print(name, "coverage", round(float(mask.mean()), 3), "max", int(alpha.max()))


if __name__ == "__main__":
    main()
