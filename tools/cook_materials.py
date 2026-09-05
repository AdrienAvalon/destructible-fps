"""Cook the bounded, offline runtime PBR library from attributed CC0 sources.

ImageMagick is used only to decode reviewed JPEG files, never by the game. Mips and the
portable package are generated here with explicit channel semantics and fixed dimensions.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path
import platform
import struct
import subprocess
import tempfile
import urllib.parse
import urllib.request
import zlib

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "assets/materials/sources.json"
PACK = ROOT / "assets/materials/industrial.pbrz"
CACHE = ROOT / "target/material-sources"
EDGE = 1024
MIPS = 11
ASSETS = (
    ("soil", "brown_mud_rocks_01"),
    ("stone", "rock_face_03"),
    ("wood", "weathered_brown_planks"),
    ("brick", "brick_wall_001"),
    ("concrete", "concrete_floor_01"),
)
CHANNEL_KEYS = {"albedo": "Diffuse", "normal": "nor_gl", "roughness": "Rough"}
SOURCE_LIMIT = 16 * 1024 * 1024
MAGIC = b"AVPBR001"
COOKER = {
    "schema": 1,
    "imagemagick": "ImageMagick 7.1.2-30 Q16-HDRI",
    "python": "3.14.7",
    "zlib": "1.3.1.zlib-ng",
}


def reviewed_origin(url):
    parsed = urllib.parse.urlsplit(url)
    if (parsed.scheme != "https" or parsed.hostname not in ("api.polyhaven.com", "dl.polyhaven.org")
            or parsed.port not in (None, 443) or parsed.username is not None or parsed.password is not None):
        raise ValueError("asset endpoint is outside the reviewed HTTPS origins")
    return parsed.hostname


class SameOriginRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Reject before urllib opens the redirected connection, not after it has fetched the data.
        if reviewed_origin(newurl) != reviewed_origin(req.full_url):
            raise ValueError("cross-origin asset redirect refused")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def download(url, limit=SOURCE_LIMIT):
    origin = reviewed_origin(url)
    request = urllib.request.Request(url, headers={"User-Agent": "AvalonDestructibleFPS/0.1"})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), SameOriginRedirects())
    with opener.open(request, timeout=45) as response:
        if reviewed_origin(response.url) != origin:
            raise ValueError("cross-origin asset redirect refused")
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError("asset exceeds the fixed download budget")
    return data


def replace_file(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        temporary.write(data)
        temporary_path = Path(temporary.name)
    temporary_path.replace(path)


def lock_sources():
    """Explicit authoring operation: resolve only the five named library entries."""
    entries = []
    for material, asset in ASSETS:
        info = json.loads(download(f"https://api.polyhaven.com/info/{asset}", 1024 * 1024))
        files = json.loads(download(f"https://api.polyhaven.com/files/{asset}", 1024 * 1024))
        sources = {}
        for channel, key in CHANNEL_KEYS.items():
            # Some older Poly Haven scans use 'diff'/'rough' rather than title-case names.
            available = files.get(key) or files.get({"Diffuse": "diff", "Rough": "rough"}.get(key, key))
            source = available["1k"]["jpg"]
            data = download(source["url"])
            if len(data) != source["size"] or hashlib.md5(data, usedforsecurity=False).hexdigest() != source["md5"]:
                raise ValueError(f"upstream size/digest mismatch for {asset}/{channel}")
            filename = f"{asset}-{channel}.jpg"
            replace_file(CACHE / filename, data)
            sources[channel] = {
                "url": source["url"], "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                "upstream_md5": source["md5"], "cache_file": filename,
            }
        dimensions = info["dimensions"][:2]
        entries.append({
            "material": material, "asset": asset, "source": f"https://polyhaven.com/a/{asset}",
            "authors": info["authors"], "tile_meters": [round(value / 1000, 4) for value in dimensions],
            "maps": sources,
        })
        print(f"Locked CC0 source: {asset}", flush=True)
    manifest = {
        "schema": 1, "edge": EDGE, "mips": MIPS, "license": "CC0-1.0",
        "license_url": "https://polyhaven.com/license", "provider": "Poly Haven",
        "cooker": COOKER,
        "materials": entries,
    }
    replace_file(MANIFEST, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode())


def source_bytes(record, fetch):
    name = record["cache_file"]
    if Path(name).name != name or not name.endswith(".jpg"):
        raise ValueError("invalid source-cache filename")
    path = CACHE / name
    if not path.exists():
        if not fetch:
            raise ValueError(f"missing {name}; use --fetch to retrieve pinned sources")
        data = download(record["url"])
    else:
        if path.stat().st_size > SOURCE_LIMIT:
            raise ValueError("oversized cached source")
        data = path.read_bytes()
    if len(data) != record["bytes"] or hashlib.sha256(data).hexdigest() != record["sha256"]:
        raise ValueError(f"pinned source mismatch for {name}")
    if not path.exists():
        replace_file(path, data)
    return path


def decode_rgb(path):
    limits = ["-limit", "memory", "128MiB", "-limit", "map", "128MiB", "-limit", "disk", "0"]
    dimensions = subprocess.check_output(
        ["magick", "identify", *limits, "-format", "%w %h", str(path)], timeout=30,
    ).decode()
    if dimensions != f"{EDGE} {EDGE}":
        raise ValueError(f"source must be exactly {EDGE} square: {path.name}")
    pixels = subprocess.check_output(
        ["magick", *limits, str(path), "+profile", "*", "-depth", "8", "rgb:-"], timeout=30,
    )
    if len(pixels) != EDGE * EDGE * 3:
        raise ValueError("decoded source has an unexpected channel count")
    return pixels


def srgb_to_linear(value):
    value /= 255
    return value / 12.92 if value <= 0.04045 else ((value + 0.055) / 1.055) ** 2.4


def unorm(value):
    return max(0, min(255, round(value * 255)))


def linear_to_srgb(value):
    return unorm(value * 12.92 if value <= 0.0031308 else 1.055 * value ** (1 / 2.4) - 0.055)


LINEAR = tuple(srgb_to_linear(value) for value in range(256))


def next_mip(color, normal, edge):
    """Box-filter energy in linear light; conserve roughness^2 and normal variance."""
    half = edge // 2
    out_color = bytearray(half * half * 4)
    out_normal = bytearray(half * half * 4)
    for y in range(half):
        for x in range(half):
            base = (y * 2 * edge + x * 2) * 4
            offsets = (base, base + 4, base + edge * 4, base + edge * 4 + 4)
            destination = (y * half + x) * 4
            for channel in range(3):
                out_color[destination + channel] = linear_to_srgb(
                    sum(LINEAR[color[index + channel]] for index in offsets) * 0.25
                )
            mean = [sum(normal[index + channel] / 127.5 - 1 for index in offsets) * 0.25 for channel in range(3)]
            length = math.sqrt(sum(component * component for component in mean))
            direction = [component / length for component in mean] if length > 1e-8 else [0, 0, 1]
            for channel in range(3):
                out_normal[destination + channel] = unorm(direction[channel] * 0.5 + 0.5)
            roughness_squared = sum((color[index + 3] / 255) ** 2 for index in offsets) * 0.25
            # Removed normal-map detail broadens the remaining specular lobe instead of shimmering.
            out_color[destination + 3] = unorm(math.sqrt(min(1, roughness_squared + max(0, 1 - length))))
            out_normal[destination + 3] = round(sum(normal[index + 3] for index in offsets) * 0.25)
    return bytes(out_color), bytes(out_normal)


def normalized_base_normals(source):
    """Canonicalize JPEG quantization and undefined below-surface normal texels."""
    normal = bytearray(len(source) // 3 * 4)
    for index in range(len(source) // 3):
        vector = [source[index * 3 + channel] / 127.5 - 1 for channel in range(3)]
        length = math.sqrt(sum(component * component for component in vector))
        if vector[2] < 0 or length < 1e-8:
            vector = [0, 0, 1]
        else:
            vector = [component / length for component in vector]
        for channel in range(3):
            normal[index * 4 + channel] = unorm(vector[channel] * 0.5 + 0.5)
    return normal


def cook(fetch=False):
    manifest = json.loads(MANIFEST.read_text())
    decoder = subprocess.check_output(["magick", "--version"], timeout=5).decode().splitlines()[0]
    if (manifest["cooker"] != COOKER or COOKER["imagemagick"] not in decoder
            or platform.python_version() != COOKER["python"] or zlib.ZLIB_RUNTIME_VERSION != COOKER["zlib"]):
        raise ValueError("cooker toolchain differs from the reviewed manifest; use the cooked pack or review a toolchain update")
    if (manifest["schema"], manifest["edge"], manifest["mips"], manifest["license"]) != (1, EDGE, MIPS, "CC0-1.0"):
        raise ValueError("material manifest is outside the reviewed runtime contract")
    entries = manifest["materials"]
    if [(entry["material"], entry["asset"]) for entry in entries] != list(ASSETS):
        raise ValueError("material layer order differs from the runtime IDs")
    colors, normals, dimensions = bytearray(), bytearray(), bytearray()
    for entry in entries:
        width, height = entry["tile_meters"]
        if not all(math.isfinite(value) and 0.25 <= value <= 16 for value in (width, height)):
            raise ValueError("material scale is outside the physical tile limits")
        dimensions.extend(struct.pack("<ff", width, height))
        maps = {channel: decode_rgb(source_bytes(record, fetch)) for channel, record in entry["maps"].items()}
        color = bytearray(EDGE * EDGE * 4)
        normal = normalized_base_normals(maps["normal"])
        for channel in range(3):
            color[channel::4] = maps["albedo"][channel::3]
        color[3::4] = maps["roughness"][0::3]
        # All five initial scanned materials are dielectric; steel keeps the procedural path.
        edge = EDGE
        while True:
            colors.extend(color)
            normals.extend(normal)
            if edge == 1:
                break
            color, normal = next_mip(color, normal, edge)
            edge //= 2
        print(f"Cooked {entry['material']}: 11 linear-light/normal-aware mips", flush=True)
    payload = colors + normals
    header = MAGIC + struct.pack("<IIII", EDGE, len(entries), MIPS, len(payload))
    digest = hashlib.sha256(header + dimensions + payload).digest()
    pack = header + digest + dimensions + zlib.compress(payload, level=9)
    print(f"Runtime pack: {len(pack):,} bytes; GPU texels: {len(payload):,} bytes", flush=True)
    return pack


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock-sources", action="store_true", help="explicitly refresh the five upstream source pins")
    parser.add_argument("--fetch", action="store_true", help="download missing sources using existing SHA256 pins")
    parser.add_argument("--check", action="store_true", help="rebuild offline and compare with the committed package")
    args = parser.parse_args()
    if args.check and args.lock_sources:
        parser.error("--check cannot refresh source pins")
    if args.lock_sources:
        lock_sources()
    data = cook(args.fetch)
    if args.check:
        if data != PACK.read_bytes():
            raise SystemExit("material package drift: regenerate with the pinned cooker environment")
        print("Material package is byte-identical")
    else:
        replace_file(PACK, data)


if __name__ == "__main__":
    main()
