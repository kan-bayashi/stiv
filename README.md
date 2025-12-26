# sivit

**S**imple **I**mage **V**iewer **I**n **T**erminal

A terminal-based image viewer with sxiv-like keybindings. Works over SSH with Tmux.

![](./samples/sivit.png)

## Features

- Kitty Graphics Protocol (KGP) image rendering
- sxiv/vim-like keyboard navigation (counts supported)
- Zlib compression for fast image transmission
- Prefetch adjacent images for instant navigation
- Render cache for snappy navigation
- `Fit` toggle (upscale to viewport) + `Normal` (shrink-only)

## Requirements

- Kitty Graphics Protocol supported terminal
- Optional: tmux (uses `allow-passthrough=on`, `sivit` attempts to set it automatically)
- Rust 1.75+

Tested: Ghostty + tmux.

## Installation

### From Release

Download the latest binary from [Releases](https://github.com/kan-bayashi/sivit/releases):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/kan-bayashi/sivit/releases/latest/download/sivit-aarch64-apple-darwin.tar.gz | tar xz
sudo mv sivit /usr/local/bin/

# Linux (x86_64)
curl -L https://github.com/kan-bayashi/sivit/releases/latest/download/sivit-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv sivit /usr/local/bin/
```

### From Source

```bash
cargo install --path .
```

## Usage

```bash
sivit image.png
sivit ~/photos/
sivit *.png
sivit ~/photos/*.jpg
```

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `Space` / `l` | Next image |
| `k` / `Backspace` / `h` | Previous image |
| `g` | First image |
| `G` | Last image |
| `f` | Toggle fit |
| `r` | Reload (clear cache) |
| `q` | Quit |

Vim-like counts are supported (e.g. `5j`, `10G`).

## Options

| Env | Default | Description |
|-----|---------|-------------|
| `SIVIT_NAV_LATCH_MS` | `150` | Navigation latch (ms) before drawing images |
| `SIVIT_RENDER_CACHE_SIZE` | `100` | Render cache entries |
| `SIVIT_PREFETCH_COUNT` | `5` | Number of images to prefetch ahead/behind |
| `SIVIT_COMPRESS_LEVEL` | `6` | Zlib compression level 0-9 |
| `SIVIT_KGP_NO_COMPRESS` | unset | Disable zlib compression |
| `SIVIT_TMUX_KITTY_MAX_PIXELS` | `2000000` | Max pixels in `Normal` mode (tmux+kitty) |
| `SIVIT_FORCE_ALT_SCREEN` | unset | Force alternate screen |
| `SIVIT_NO_ALT_SCREEN` | unset | Disable alternate screen |

## Contributing

See `CONTRIBUTING.md`.

## References

- [yazi](https://github.com/sxyazi/yazi) - Kitty Graphics Protocol implementation reference
- [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)

## License

MIT
