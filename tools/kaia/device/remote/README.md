# Remote access to a test Kobo

Tools the developer used on 2026-10-14 to drive a Kobo Libra Colour over Wi-Fi during device tests. They are for test devices only and are not part of the Kaia package.

- `nickelmenu-kaia-remote`: NickelMenu entries. "Kaia remote on" turns on Nickel's `force_wifi` setting, connects Wi-Fi and starts the dropbear server that ships in `.adds/kaia` on port 2222 with password logins off (`-s`). It reads keys from `.adds/kaia/settings/SSH/authorized_keys` and keeps its host keys in that folder; nothing is written to the system partition. "Kaia remote off" stops the server and turns `force_wifi` off. "Rescan books" runs Nickel's full rescan.
- `touch.lua`: synthetic finger touches (tap, hold, swipe, pinch) for the Elan touch screen, run with Kaia's `luajit`. Coordinates are portrait screen pixels.
- `keys.lua`: presses the page buttons (193 back, 194 forward, or both, which exits the Rust app).
- `fb2png.py`: converts a `/dev/fb0` dump (RGBA, 1264 x 1680, stride 5056) to PNG with the Python standard library.
- `fbprobe.py`: share of dark pixels in a region of the last dump, to check a step without viewing it.

Use your own key pair, copy only the public key to the device, and turn remote access off when done. Nickel list rows need a press of about 180 ms; icons take a shorter tap. Check the screen after each step: a stray tap at the top-right corner of a book page adds a bookmark.
