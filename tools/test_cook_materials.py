"""Offline numerical regressions for the material asset contract."""

import math
import unittest
import urllib.request

from cook_materials import SameOriginRedirects, next_mip, normalized_base_normals, reviewed_origin


class MaterialMipTests(unittest.TestCase):
    def test_srgb_color_is_filtered_in_linear_light_but_roughness_is_linear(self):
        color = bytes([0, 0, 0, 0, 255, 255, 255, 255] * 2)
        normal = bytes([128, 128, 255, 0] * 4)
        filtered, _ = next_mip(color, normal, 2)
        self.assertTrue(all(187 <= value <= 188 for value in filtered[:3]))
        self.assertAlmostEqual(filtered[3] / 255, math.sqrt(0.5), delta=0.005)

    def test_removed_normal_variance_broadens_roughness(self):
        color = bytes([100, 120, 140, 30] * 4)
        normal = bytes([230, 128, 204, 0, 25, 128, 204, 0] * 2)
        filtered, filtered_normal = next_mip(color, normal, 2)
        self.assertGreater(filtered[3], 150)
        self.assertEqual(filtered_normal, bytes([128, 128, 255, 0]))

    def test_undefined_normals_become_flat_and_valid_normals_are_unit_length(self):
        normal = normalized_base_normals(bytes([0, 0, 0, 180, 150, 230]))
        self.assertEqual(normal[:4], bytes([128, 128, 255, 0]))
        vector = [value / 127.5 - 1 for value in normal[4:7]]
        self.assertAlmostEqual(sum(value * value for value in vector), 1, delta=0.012)


class MaterialDownloadTests(unittest.TestCase):
    def test_only_public_reviewed_https_origins_are_accepted(self):
        for url in ("http://dl.polyhaven.org/a", "https://example.invalid/a",
                    "https://dl.polyhaven.org:8443/a", "https://name@dl.polyhaven.org/a",
                    "https://127.0.0.1/a", "file:///tmp/a.jpg"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                reviewed_origin(url)
        self.assertEqual(reviewed_origin("https://dl.polyhaven.org:443/a"), "dl.polyhaven.org")

    def test_redirects_fail_before_another_origin_or_plaintext_request_is_opened(self):
        handler = SameOriginRedirects()
        request = urllib.request.Request("https://dl.polyhaven.org/a")
        for target in ("https://api.polyhaven.com/b", "http://dl.polyhaven.org/b",
                       "https://example.invalid/b", "https://127.0.0.1/b"):
            with self.subTest(target=target), self.assertRaises(ValueError):
                handler.redirect_request(request, None, 302, "Found", {}, target)

    def test_same_origin_https_redirect_is_preserved_without_network_io(self):
        handler = SameOriginRedirects()
        request = urllib.request.Request("https://dl.polyhaven.org/a")
        result = handler.redirect_request(request, None, 302, "Found", {}, "https://dl.polyhaven.org/b")
        self.assertEqual(result.full_url, "https://dl.polyhaven.org/b")


if __name__ == "__main__":
    unittest.main()
