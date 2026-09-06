"""Offline numerical/orientation and hostile-input checks for the actual IBL cooker."""

from array import array
import math
import struct
import unittest

import cook_environment as env


def rgbe_fixture():
    header = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 8\n"
    return header + bytes([2, 2, 0, 8, 136, 128, 136, 64, 136, 32, 136, 130])


class EnvironmentCookerTests(unittest.TestCase):
    def test_rgbe_is_linear_hdr_not_srgb_or_clipped(self):
        self.assertEqual(list(env.decode_rgbe(rgbe_fixture(), 8, 1)), [2.0, 1.0, 0.5] * 8)

    def test_literal_and_zero_exponent(self):
        base = rgbe_fixture().split(b"\n-Y")[0] + b"\n-Y 1 +X 8\n" + bytes([2, 2, 0, 8])
        literal = b"".join(bytes([8]) + bytes([channel]) * 8 for channel in (128, 64, 32, 130))
        self.assertEqual(env.decode_rgbe(base + literal, 8, 1), env.decode_rgbe(rgbe_fixture(), 8, 1))
        self.assertEqual(list(env.decode_rgbe(rgbe_fixture()[:-1] + b"\0", 8, 1)), [0.0] * 24)

    def test_corrupt_or_unbounded_rgbe_is_rejected(self):
        good = rgbe_fixture()
        variants = [good[:i] for i in range(len(good))]
        variants += [good + b"trailing", good.replace(b"rgbe", b"xyze"),
                     good.replace(b"-Y 1 +X 8", b"+Y 1 +X 8"),
                     good.replace(b"+X 8", b"+X 9"),
                     good.replace(b"\x88\x80", b"\x89\x80"),  # scanline overflow
                     good.replace(b"\x88\x80", b"\0\x80"),
                     good[:-1] + b"\xff", good.replace(b"#?RADIANCE", b"#?RGBE"),
                     good.replace(b"FORMAT", b"EXPOSURE=2\nFORMAT"),
                     good.replace(b"FORMAT", b"COLORCORR=1 2 1\nFORMAT"),
                     good.replace(b"FORMAT", b"PRIMARIES=1\nFORMAT"),
                     good.replace(b"FORMAT", b"FORMAT=32-bit_rle_xyze\nFORMAT"),
                     good.replace(b"FORMAT", b"#" + b"A" * 1024 + b"\nFORMAT")]
        for data in variants:
            with self.subTest(length=len(data)), self.assertRaises(ValueError):
                env.decode_rgbe(data, 8, 1)
        for width, height in [(7, 1), (1025, 1), (8, 0), (8, 513)]:
            with self.assertRaises(ValueError):
                env.decode_rgbe(good, width, height)

    def test_cube_centers_and_asymmetric_face_axes(self):
        centers = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))
        for face, center in enumerate(centers):
            self.assertEqual(env.cube_direction(face, 0, 0), center)
        expected = ((1, -0.25, -0.5), (-1, -0.25, 0.5), (0.5, 1, 0.25),
                    (0.5, -1, -0.25), (0.5, -0.25, 1), (-0.5, -0.25, -1))
        for face, direction in enumerate(expected):
            self.assertEqual(env.cube_direction(face, 0.5, 0.25), env.normalize(direction))
        # Every face-edge midpoint has exactly one partner with identical world direction.
        edges = [env.cube_direction(f, u, v) for f in range(6) for u, v in ((-1, 0), (1, 0), (0, -1), (0, 1))]
        self.assertTrue(all(edges.count(edge) == 2 for edge in edges))

    def test_latlong_wrap_poles_and_asymmetric_direction_field(self):
        pixels = array("f", [value for y in range(4) for x in range(8) for value in (x, y, 0)])
        self.assertEqual(env.latlong_sample(pixels, (1, 0, 0), 8, 4), (3.5, 1.5, 0))
        self.assertEqual(env.latlong_sample(pixels, (0, 0, 1), 8, 4), (5.5, 1.5, 0))
        self.assertEqual(env.latlong_sample(pixels, (0, 0, -1), 8, 4), (1.5, 1.5, 0))
        self.assertEqual(env.latlong_sample(pixels, (0, 1, 0), 8, 4)[1], 0)
        self.assertEqual(env.latlong_sample(pixels, (0, -1, 0), 8, 4)[1], 3)
        a = env.latlong_sample(pixels, env.normalize((-1, 0, 1e-9)), 8, 4)
        b = env.latlong_sample(pixels, env.normalize((-1, 0, -1e-9)), 8, 4)
        for x, y in zip(a, b):
            self.assertAlmostEqual(x, y, places=7)

    def test_constant_hdr_is_preserved_by_both_convolutions(self):
        for roughness in (None, 0, 0.01, 0.5, 1):
            directions = env.hemisphere_samples(128, roughness)
            for n in ((0, 1, 0), (0, 0, -1), env.normalize((1, 2, 3))):
                actual = env.convolve(lambda _: (2, 0.5, 0.125), n, directions)
                for a, b in zip(actual, (2, 0.5, 0.125)):
                    self.assertAlmostEqual(a, b, places=12)

    def test_diffuse_cosine_integral_has_exact_pi_normalization(self):
        # L=cos(theta) -> E/pi=2/3 on the upper hemisphere.
        actual = env.convolve(lambda n: (max(0, n[1]),) * 3, (0, 1, 0), env.hemisphere_samples(4096))
        self.assertAlmostEqual(actual[0], 2 / 3, delta=0.0003)

    def test_split_sum_mirror_limit_and_rough_endpoints(self):
        for ndotv in (0.05, 0.5, 1):
            a, b, _ = env.integrate_brdf(ndotv, 0)
            self.assertAlmostEqual(a, 1 - (1 - ndotv) ** 5, places=10)
            self.assertAlmostEqual(b, (1 - ndotv) ** 5, places=10)
        a, b, _ = env.integrate_brdf(1, 1, 8192)
        self.assertAlmostEqual(a, 0.3068, delta=0.0003)
        self.assertLess(b, 0.001)
        for roughness in (0.01, 0.5, 1):
            for ndotv in (0.001, 0.5, 1):
                self.assertTrue(all(math.isfinite(x) and 0 <= x <= 1.01 for x in env.integrate_brdf(ndotv, roughness)))

    def test_half_layout_and_range(self):
        self.assertEqual(struct.unpack("<4e", env.half_texel((2, 0.5, 0.25))), (2, 0.5, 0.25, 1))
        for value in (-1, float("inf"), float("nan"), 65505):
            with self.assertRaises(ValueError):
                env.half_texel((value, 0, 0))
        self.assertEqual(len(env.cube_bytes(4, lambda _: (1, 2, 3))), 6 * 4 * 4 * 8)


if __name__ == "__main__":
    unittest.main()
