"""Deterministic controls for the signed-fixture converter; standard library only."""
from pathlib import Path
import tempfile
import unittest

from make_incremental_pdf import build_ber, to_indefinite


class SignaturePaddingTests(unittest.TestCase):
    def test_every_final_byte_survives_reserved_padding(self):
        with tempfile.TemporaryDirectory() as folder:
            source, target = (Path(folder) / name for name in ("source.pdf", "target.pdf"))
            prefix, suffix = b"%PDF-1.7\n/Contents <", b"> /ByteRange [0 20 90 30]\n%%EOF"
            for last in range(256):
                with self.subTest(last=last):
                    # SEQUENCE containing an OCTET STRING, with a genuine zero
                    # ending in one case. Expected BER is authored independently.
                    der = b"\x30\x04\x04\x02\x01" + bytes([last])
                    ber = b"\x30\x80\x04\x02\x01" + bytes([last]) + b"\x00\x00"
                    for upper in (False, True):
                        digits = (der + bytes(32)).hex()
                        encoded = (ber + bytes(30)).hex()
                        if upper:
                            digits, encoded = digits.upper(), encoded.upper()
                        source.write_bytes(prefix + digits.encode("ascii") + suffix)
                        self.assertTrue(build_ber(str(source), str(target)))
                        self.assertEqual(target.read_bytes(), prefix + encoded.encode("ascii") + suffix)
                        self.assertEqual(source.stat().st_size, target.stat().st_size)

    def test_outer_lengths_preserve_zero_payload_and_reject_nonzero_padding(self):
        for payload in (b"\0", bytes(128), bytes(260)):
            length = len(payload)
            encoded = length.to_bytes((length.bit_length() + 7) // 8, "big")
            tag = b"\x04" + (bytes([length]) if length < 128 else bytes([0x80 | len(encoded)]) + encoded)
            der = tag + payload
            self.assertEqual(to_indefinite(der), der)
            self.assertEqual(to_indefinite(der + bytes(32), padded=True), der)
            with self.assertRaisesRegex(ValueError, "trailing bytes"):
                to_indefinite(der + bytes(32))
            with self.assertRaisesRegex(ValueError, "trailing bytes"):
                to_indefinite(der + b"\0\x01", padded=True)
            with self.assertRaisesRegex(ValueError, "truncated"):
                to_indefinite(der[:-1], padded=True)

    def test_invalid_or_unreserved_input_does_not_write_output(self):
        with tempfile.TemporaryDirectory() as folder:
            source, target = (Path(folder) / name for name in ("source.pdf", "target.pdf"))
            for contents in (b"/NoSignature", b"/Contents <00000000>", b"/Contents <3003040100>"):
                source.write_bytes(contents)
                self.assertFalse(build_ber(str(source), str(target)))
                self.assertFalse(target.exists())
            source.write_bytes(b"/Contents <30030401000100000000>")
            with self.assertRaisesRegex(ValueError, "trailing bytes"):
                build_ber(str(source), str(target))
            self.assertFalse(target.exists())


if __name__ == "__main__":
    unittest.main()
