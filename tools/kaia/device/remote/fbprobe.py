"""Print how dark a region of the last capture is: fbprobe.py X0 Y0 X1 Y1 -> share of dark pixels."""
import sys
x0, y0, x1, y1 = map(int, sys.argv[1:5])
d = open('/tmp/kobo-fb.raw', 'rb').read()
s = 5056
n = dark = 0
for y in range(y0, y1):
    for x in range(x0, x1, 2):
        n += 1
        dark += d[y*s + x*4] < 100
print(round(dark / n, 3))
