"""Convert a raw Kobo Libra Colour framebuffer dump (RGBA, 1264x1680, stride 5056) to PNG."""
import sys, zlib, struct
src, dst = sys.argv[1], sys.argv[2]
w, h, stride = 1264, 1680, 5056
d = open(src, 'rb').read()
rows = bytearray()
for y in range(h):
    row = d[y*stride:y*stride+w*4]
    rgb = bytearray(w*3)
    rgb[0::3] = row[0::4]; rgb[1::3] = row[1::4]; rgb[2::3] = row[2::4]
    rows += b'\x00' + rgb
def chunk(t, b):
    return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t+b) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(bytes(rows), 6)) + chunk(b'IEND', b'')
open(dst, 'wb').write(png)
