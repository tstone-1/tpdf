"""Writes headers.json, the image headers both signature readers are tested on.

Run from the repository root: python3 src-tauri/testdata/signature/headers.py
Each case is a name, the file as hex strings and counts of zero bytes, and the
size the header must read as, or null when it must be refused. The readers are
`signatureDimensions` in src/lib/signature.ts and `dimensions` in
src-tauri/src/signature_import.rs; each has a test that asserts the number of
cases, so raise both counts when adding one.
"""
import json, struct
SIG = bytes([137,80,78,71,13,10,26,10])
def chunk(kind, data=b'', length=None):
    return struct.pack('>I', len(data) if length is None else length) + kind + data + b'\0\0\0\0'
def ihdr(w, h): return chunk(b'IHDR', struct.pack('>II', w, h) + bytes([8,6,0,0,0]))
def png(w, h, *middle, end=chunk(b'IEND'), data=chunk(b'IDAT')):
    return [SIG + ihdr(w, h), *middle, data + end]
def sof(w, h, marker=0xc0, precision=8):
    return bytes([255, marker, 0, 11, precision]) + struct.pack('>HH', h, w) + bytes([1,1,17,0])
SOS = bytes([255,218,0,8,1,1,0,0,63,0])
SCAN = bytes([12,255,0,22])
EOI = bytes([255,217])
def jpeg(w, h, **k): return [b'\xff\xd8' + sof(w, h, **k) + SOS + SCAN + EOI]
MB = 10 * 1024 * 1024
def padded(total):
    n = total - 57 - 12
    return [SIG + ihdr(4, 4) + struct.pack('>I', n) + b'tEXt', n, b'\0\0\0\0' + chunk(b'IDAT') + chunk(b'IEND')]
def commented(total):
    base = len(jpeg(4, 4)[0])
    out = [b'\xff\xd8']
    left = total - base
    while left:
        n = min(left, 65537) ; assert n >= 4 and (left - n == 0 or left - n >= 4)
        out += [bytes([255, 254]) + struct.pack('>H', n - 2), n - 4]
        left -= n
    return out + [sof(4, 4) + SOS + SCAN + EOI]
V = [
 ('a PNG at the longest side', png(8192, 1024), [8192, 1024]),
 ('a PNG at the most pixels', png(4096, 2048), [4096, 2048]),
 ('a PNG one pixel wide', png(1, 1), [1, 1]),
 ('a PNG a pixel too wide', png(8193, 1), None),
 ('a PNG a pixel too tall', png(1, 8193), None),
 ('a PNG a row over the most pixels', png(4096, 2049), None),
 ('a PNG with no width', png(0, 1), None),
 ('a PNG with no height', png(1, 0), None),
 ('an animated PNG', png(1, 1, chunk(b'acTL', bytes(8))), None),
 ('a PNG with a second header', png(1, 1, ihdr(1, 1)), None),
 ('a PNG without image data', png(1, 1, data=b''), None),
 ('a PNG without its end', png(1, 1, end=b''), None),
 ('a PNG whose end carries data', png(1, 1, end=chunk(b'IEND', b'\0')), None),
 ('a PNG with a byte after its end', png(1, 1, end=chunk(b'IEND') + b'\0'), None),
 ('a PNG cut short', [(SIG + ihdr(1, 1) + chunk(b'IDAT') + chunk(b'IEND'))[:-1]], None),
 ('a PNG whose chunk runs past the file', png(1, 1, chunk(b'tEXt', b'ab', length=4000)), None),
 ('a PNG whose header chunk is the wrong length', [SIG + chunk(b'IHDR', struct.pack('>II', 1, 1) + bytes(4)) + bytes(1) + chunk(b'IDAT') + chunk(b'IEND')], None),
 ('a PNG of exactly 10 MiB', padded(MB), [4, 4]),
 ('a PNG a byte over 10 MiB', padded(MB + 1), None),
 ('a JPEG', jpeg(40, 20), [40, 20]),
 ('a progressive JPEG', jpeg(40, 20, marker=0xc2), [40, 20]),
 ('an extended sequential JPEG', jpeg(40, 20, marker=0xc1), [40, 20]),
 ('a JPEG at the longest side', jpeg(8192, 1024), [8192, 1024]),
 ('a JPEG a pixel too wide', jpeg(8193, 1), None),
 ('a JPEG a pixel too tall', jpeg(1, 8193), None),
 ('a JPEG a row over the most pixels', jpeg(4096, 2049), None),
 ('a JPEG with no height', jpeg(40, 0), None),
 ('a lossless JPEG', jpeg(40, 20, marker=0xc3), None),
 ('an arithmetic-coded JPEG', jpeg(40, 20, marker=0xc9), None),
 ('a twelve-bit JPEG', jpeg(40, 20, precision=12), None),
 ('a JPEG with two frames', [b'\xff\xd8' + sof(40, 20) + sof(40, 20) + SOS + SCAN + EOI], None),
 ('a JPEG with a scan before its frame', [b'\xff\xd8' + SOS + SCAN + sof(40, 20) + EOI], None),
 ('a JPEG without a scan', [b'\xff\xd8' + sof(40, 20) + EOI], None),
 ('a JPEG without its end', [b'\xff\xd8' + sof(40, 20) + SOS + SCAN], None),
 ('a JPEG with a byte after its end', [b'\xff\xd8' + sof(40, 20) + SOS + SCAN + EOI + b'\0'], None),
 ('a JPEG that changes its height late', [b'\xff\xd8' + sof(40, 20) + SOS + SCAN + bytes([255,220,0,4,255,255]) + EOI], None),
 ('a JPEG with restart markers and fill bytes', [b'\xff\xd8' + sof(40, 20) + SOS + SCAN + bytes([255,208,7,255,255,215,9]) + EOI], [40, 20]),
 ('a JPEG whose segment runs past the file', [b'\xff\xd8' + bytes([255,254,0,200]) + sof(40, 20) + SOS + SCAN + EOI], None),
 ('a JPEG whose segment is shorter than its length', [b'\xff\xd8' + bytes([255,254,0,1]) + sof(40, 20) + SOS + SCAN + EOI], None),
 ('a JPEG that ends inside a marker', [b'\xff\xd8' + sof(40, 20) + SOS + SCAN + b'\xff'], None),
 ('a JPEG that ends inside a length', [b'\xff\xd8' + sof(40, 20) + bytes([255,254,0])], None),
 ('a JPEG of exactly 10 MiB', commented(MB), [4, 4]),
 ('a JPEG a byte over 10 MiB', commented(MB + 1), None),
 ('a GIF', [b'GIF89a' + bytes(40)], None),
 ('a PNG signature and nothing else', [SIG], None),
 ('an empty file', [], None),
]
def total(parts): return sum(p if isinstance(p, int) else len(p) for p in parts)
assert total(padded(MB)) == MB and total(commented(MB)) == MB and total(commented(MB+1)) == MB+1
names = [n for n, _, _ in V]; assert len(set(names)) == len(names)
lines = []
for name, parts, size in V:
    lines.append('  ' + json.dumps({'name': name, 'parts': [p if isinstance(p, int) else p.hex() for p in parts], 'size': size}))
open('src-tauri/testdata/signature/headers.json', 'w', newline='').write('[\n' + ',\n'.join(lines) + '\n]\n')
print(len(V), sum(1 for v in V if v[2]))
