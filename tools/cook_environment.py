"""Offline, bounded CC0 HDR -> linear half-float sky/IBL pack; no runtime decoder.

Only the reviewed modern RGBE RLE layout is accepted. This is not a general HDR importer.
Cube faces use WebGPU order +X,-X,+Y,-Y,+Z,-Z, with top-origin pixel rows.
"""

import argparse
from array import array
import hashlib
import io
import json
import math
from pathlib import Path
import platform
import struct
import zlib

from cook_materials import download, replace_file

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "assets/environment/source.json"
PACK = ROOT / "assets/environment/overcast.iblz"
CACHE = ROOT / "target/environment-source/overcast_soil_puresky_1k.hdr"
ASSET = "overcast_soil_puresky"
SOURCE_LIMIT = 2 * 1024 * 1024
WIDTH, HEIGHT = 1024, 512
SKY_EDGE, DIFFUSE_EDGE, SPECULAR_EDGE, SPECULAR_MIPS, LUT_EDGE = 256, 16, 64, 7, 64
SOURCE_GAIN = 1.0
COOKER = {"schema": 1, "python": "3.14.7", "zlib": "1.3.1.zlib-ng"}
HEADER = struct.pack("<8s6I", b"AVIBL001", SKY_EDGE, DIFFUSE_EDGE, SPECULAR_EDGE,
                     SPECULAR_MIPS, LUT_EDGE, 3_452_912)


def lock_source():
    info = json.loads(download(f"https://api.polyhaven.com/info/{ASSET}", 1024 * 1024))
    files = json.loads(download(f"https://api.polyhaven.com/files/{ASSET}", 1024 * 1024))
    record = files["hdri"]["1k"]["hdr"]
    data = download(record["url"], SOURCE_LIMIT)
    if (len(data) != record["size"]
            or hashlib.md5(data, usedforsecurity=False).hexdigest() != record["md5"]):
        raise ValueError("upstream HDR size/digest mismatch")
    decode_rgbe(data)
    manifest = {
        "schema": 1, "asset": ASSET, "source": f"https://polyhaven.com/a/{ASSET}",
        "authors": info["authors"], "license": "CC0-1.0", "license_url": "https://polyhaven.com/license",
        "url": record["url"], "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
        "upstream_md5": record["md5"], "width": WIDTH, "height": HEIGHT,
        "source_gain": SOURCE_GAIN, "cooker": COOKER,
    }
    replace_file(CACHE, data)
    replace_file(MANIFEST, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode())


def load_source(fetch):
    if MANIFEST.stat().st_size > 8192:
        raise ValueError("oversized environment manifest")
    manifest = json.loads(MANIFEST.read_text())
    if (manifest["schema"] != 1 or manifest["asset"] != ASSET
            or manifest["width"] != WIDTH or manifest["height"] != HEIGHT
            or manifest["source_gain"] != SOURCE_GAIN or manifest["cooker"] != COOKER
            or not 1 <= manifest["bytes"] <= SOURCE_LIMIT):
        raise ValueError("unsupported environment source contract")
    if CACHE.exists():
        if CACHE.stat().st_size > SOURCE_LIMIT:
            raise ValueError("oversized cached HDR")
        data = CACHE.read_bytes()
    elif fetch:
        data = download(manifest["url"], SOURCE_LIMIT)
    else:
        raise ValueError("missing reviewed HDR; use --fetch")
    if len(data) != manifest["bytes"] or hashlib.sha256(data).hexdigest() != manifest["sha256"]:
        raise ValueError("pinned HDR source mismatch")
    pixels = decode_rgbe(data)
    if not CACHE.exists():
        replace_file(CACHE, data)
    return pixels


def decode_rgbe(data, width=WIDTH, height=HEIGHT):
    """Exact, bounded modern RLE input. Tiny dimensions are exposed only for offline tests."""
    if not (8 <= width <= WIDTH and 1 <= height <= HEIGHT and len(data) <= SOURCE_LIMIT):
        raise ValueError("unsupported RGBE dimensions or size")
    stream = io.BytesIO(data)
    if stream.readline(64) != b"#?RADIANCE\n":
        raise ValueError("unsupported RGBE magic")
    header = []
    while True:
        line = stream.readline(1025)
        if not line or stream.tell() > 1024:
            raise ValueError("invalid or oversized RGBE header")
        if line == b"\n":
            break
        if line.startswith((b"EXPOSURE=", b"COLORCORR=", b"PRIMARIES=")):
            raise ValueError("unsupported RGBE color/exposure metadata")
        if line.startswith(b"FORMAT=") and line != b"FORMAT=32-bit_rle_rgbe\n":
            raise ValueError("unsupported or conflicting RGBE encoding")
        header.append(line)
    if header.count(b"FORMAT=32-bit_rle_rgbe\n") != 1:
        raise ValueError("unsupported RGBE encoding")
    if stream.readline(64) != f"-Y {height} +X {width}\n".encode():
        raise ValueError("unsupported RGBE orientation/dimensions")
    pixels = array("f")
    for _ in range(height):
        if stream.read(4) != bytes([2, 2, width >> 8, width & 255]):
            raise ValueError("unsupported RGBE scanline")
        channels = []
        for _ in range(4):
            channel = bytearray()
            while len(channel) < width:
                raw_count = stream.read(1)
                if not raw_count or raw_count[0] == 0:
                    raise ValueError("truncated or zero-length RGBE run")
                code = raw_count[0]
                count = code - 128 if code > 128 else code
                if count > width - len(channel):
                    raise ValueError("RGBE run exceeds scanline")
                value = stream.read(1 if code > 128 else count)
                if len(value) != (1 if code > 128 else count):
                    raise ValueError("truncated RGBE channel")
                channel.extend(value * count if code > 128 else value)
            channels.append(channel)
        for x in range(width):
            exponent = channels[3][x]
            scale = math.ldexp(SOURCE_GAIN, exponent - 136) if exponent else 0.0
            rgb = [channels[c][x] * scale for c in range(3)]
            if any(not math.isfinite(v) or v > 65504 for v in rgb):
                raise ValueError("HDR radiance outside finite half-float range")
            pixels.extend(rgb)
    if stream.read(1):
        raise ValueError("trailing RGBE payload")
    return pixels


def normalize(v):
    length = math.sqrt(sum(x * x for x in v))
    return tuple(x / length for x in v)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def cube_direction(face, u, v):
    return normalize(((1, -v, -u), (-1, -v, u), (u, 1, v), (u, -1, -v),
                      (u, -v, 1), (-u, -v, -1))[face])


def latlong_sample(pixels, direction, width=WIDTH, height=HEIGHT):
    x, y, z = direction
    u = (math.atan2(z, x) / math.tau + 0.5) * width - 0.5
    v = math.acos(max(-1, min(1, y))) / math.pi * height - 0.5
    ix, iy = math.floor(u), math.floor(v)
    tx, ty = u - ix, v - iy
    a = (max(0, min(height - 1, iy)) * width + ix % width) * 3
    b = (max(0, min(height - 1, iy)) * width + (ix + 1) % width) * 3
    c = (max(0, min(height - 1, iy + 1)) * width + ix % width) * 3
    d = (max(0, min(height - 1, iy + 1)) * width + (ix + 1) % width) * 3
    return tuple((pixels[a + k] * (1 - tx) + pixels[b + k] * tx) * (1 - ty)
                 + (pixels[c + k] * (1 - tx) + pixels[d + k] * tx) * ty for k in range(3))


def hammersley(index, count):
    bits, result, weight = index, 0.0, 0.5
    while bits:
        result += (bits & 1) * weight
        bits >>= 1
        weight *= 0.5
    return index / count, result


def ggx_half(u, v, roughness):
    alpha = roughness * roughness
    cosine = math.sqrt((1 - v) / (1 + (alpha * alpha - 1) * v))
    sine = math.sqrt(max(0, 1 - cosine * cosine))
    return math.cos(math.tau * u) * sine, math.sin(math.tau * u) * sine, cosine


def hemisphere_samples(count, roughness=None):
    result = []
    for i in range(count):
        u, v = hammersley(i, count)
        if roughness is None:
            # Cosine-weighted estimator returns E/pi, not irradiance E.
            result.append((math.cos(math.tau * u) * math.sqrt(v),
                           math.sin(math.tau * u) * math.sqrt(v), math.sqrt(1 - v), 1.0))
        else:
            hx, hy, hz = ggx_half(u, v, roughness)
            lx, ly, lz = 2 * hz * hx, 2 * hz * hy, 2 * hz * hz - 1
            if lz > 0:
                result.append((lx, ly, lz, lz))
    return result


def convolve(sample, normal, directions):
    tangent = normalize(cross((0, 0, 1) if abs(normal[2]) < 0.999 else (1, 0, 0), normal))
    bitangent = cross(normal, tangent)
    total = [0.0, 0.0, 0.0]
    weight_sum = 0.0
    for x, y, z, weight in directions:
        direction = tuple(tangent[i] * x + bitangent[i] * y + normal[i] * z for i in range(3))
        color = sample(direction)
        for k in range(3):
            total[k] += color[k] * weight
        weight_sum += weight
    return tuple(v / weight_sum for v in total)


def integrate_brdf(ndotv, roughness, count=256):
    vx = math.sqrt(max(0, 1 - ndotv * ndotv))
    k = roughness * roughness * 0.5
    a, b = 0.0, 0.0
    for i in range(count):
        hx, _, hz = ggx_half(*hammersley(i, count), roughness)
        vdoth = max(0, vx * hx + ndotv * hz)
        ndotl = max(0, 2 * vdoth * hz - ndotv)
        if ndotl > 0:
            gv = ndotv / max(ndotv * (1 - k) + k, 1e-8)
            gl = ndotl / max(ndotl * (1 - k) + k, 1e-8)
            visibility = gv * gl * vdoth / max(hz * ndotv, 1e-8)
            fresnel = (1 - vdoth) ** 5
            a += (1 - fresnel) * visibility
            b += fresnel * visibility
    return a / count, b / count, 0.0


def half_texel(rgb):
    if any(not math.isfinite(v) or not 0 <= v <= 65504 for v in rgb):
        raise ValueError("non-finite or out-of-range cooked HDR")
    return struct.pack("<4e", *rgb, 1.0)


def cube_bytes(edge, sample):
    output = bytearray()
    for face in range(6):
        for y in range(edge):
            for x in range(edge):
                output.extend(half_texel(sample(cube_direction(face, (x + 0.5) * 2 / edge - 1,
                                                              (y + 0.5) * 2 / edge - 1))))
    return output


def cook(pixels):
    # Pure-sky lower hemispheres are not local terrain. A documented dark Lambertian
    # ground approximation keeps upward-facing reflections from seeing a second sky.
    sky_sample = lambda direction: latlong_sample(pixels, direction)
    ground_light = convolve(sky_sample, (0, 1, 0), hemisphere_samples(1024))
    ground = tuple(a * b for a, b in zip(ground_light, (0.12, 0.10, 0.08)))

    def sample(direction):
        blend = max(0, min(1, -direction[1] / 0.08))
        sky = sky_sample(direction)
        return tuple(a * (1 - blend) + b * blend for a, b in zip(sky, ground))

    payload = cube_bytes(SKY_EDGE, sample)
    print("Sky radiance cooked", flush=True)
    diffuse = hemisphere_samples(512)
    payload.extend(cube_bytes(DIFFUSE_EDGE, lambda normal: convolve(sample, normal, diffuse)))
    print("Diffuse E/pi cooked", flush=True)
    # mip-major ordering is explicit at the Rust upload boundary.
    for mip in range(SPECULAR_MIPS):
        directions = hemisphere_samples(256, mip / (SPECULAR_MIPS - 1))
        payload.extend(cube_bytes(SPECULAR_EDGE >> mip,
                                 sample if mip == 0 else lambda normal: convolve(sample, normal, directions)))
        print(f"Specular roughness mip {mip} cooked", flush=True)
    for y in range(LUT_EDGE):
        for x in range(LUT_EDGE):
            payload.extend(half_texel(integrate_brdf((x + 0.5) / LUT_EDGE, (y + 0.5) / LUT_EDGE)))
    if len(payload) != struct.unpack_from("<I", HEADER, 28)[0]:
        raise ValueError(f"cooked layout mismatch: {len(payload)}")
    return HEADER + hashlib.sha256(HEADER + payload).digest() + zlib.compress(payload, 9)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock-source", action="store_true")
    parser.add_argument("--fetch", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check and args.lock_source:
        parser.error("--check cannot update the source lock")
    if platform.python_version() != COOKER["python"] or zlib.ZLIB_RUNTIME_VERSION != COOKER["zlib"]:
        raise ValueError("authoring toolchain differs from the reviewed contract")
    if args.lock_source:
        lock_source()
    pack = cook(load_source(args.fetch))
    if args.check:
        if not PACK.exists() or PACK.read_bytes() != pack:
            raise ValueError("cooked environment is not byte-for-byte reproducible")
        print("Environment pack verified offline")
    else:
        replace_file(PACK, pack)
        print(f"Environment pack: {len(pack)} bytes")


if __name__ == "__main__":
    main()
