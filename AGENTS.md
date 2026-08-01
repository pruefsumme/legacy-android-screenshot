# Technical notes

This repository contains a small command-line program for screenshots from
old Android devices whose ADB shell predates `exec-out` and usually has no
`screencap` binary.

## Capture pipeline

1. Resolve the ADB serial, or require `--serial` when several devices are
   connected.
2. Read framebuffer metadata from `/sys/class/graphics/<fb>/`:
   `virtual_size`, `modes`, `bits_per_pixel`, and `stride`.
3. Prefer the physical height from `modes`; fall back to half the virtual
   height for common double-buffered framebuffers.
4. Run the device’s `dd` to copy exactly one visible-height frame into
   `/data/local/tmp`.
5. Apply mode `0644`. Some old Android toolbox versions create the file with
   no read bit even though the shell user owns it.
6. Pull the file with `adb pull`. ADB file sync is binary-safe even when the
   device only supports a PTY for `adb shell`.
7. Decode the configured pixel layout into RGBA and write a PNG.
8. Remove the remote and host temporary files on both success and failure.

Directly streaming `/dev/graphics/fb0` through `adb shell` must not be used:
legacy PTY handling can translate line feeds and corrupt binary pixels.

## Supported pixel layouts

`--format auto` selects the common layout from the framebuffer depth:

- 16 bpp: little-endian RGB565
- 24 bpp: RGB888
- 32 bpp: RGBA8888

The CLI also accepts `rgba8888`, `bgra8888`, `rgb565`, `bgr565`, and `rgb888`
for devices that need an override. The HTC Desire capture uses 32-bit RGBA
pixels and a 480×800 physical mode over a 480×1600 virtual framebuffer.

## Development

Use the offline cache when working in a restricted environment:

```sh
cargo fmt --all
cargo check --offline
cargo test --offline
```

The unit tests cover mode parsing, RGB565 conversion, and row stride handling.
End-to-end testing requires a connected legacy Android device with readable
framebuffer access. The repository’s README example image was captured from
the HTC Desire used during development. The README flowchart is intentionally
a text-only SVG so labels stay exact and remain readable without rasterized
illustrations.

Keep the public README short and practical. Put protocol details, device
quirks, and implementation changes here.

## Installer

`install.sh` is the supported Linux installation path. It builds with
`cargo build --release --locked` and installs the binary with mode `0755`.
The default destination is `$HOME/.local/bin`; `PREFIX` can be set for a
system-wide destination, for example `PREFIX=/usr/local`.

Keep the installer dependency-light: it should use tools normally present on
a Linux development system (`bash`, `cargo`, and `install`) and should not
silently invoke `sudo`.
