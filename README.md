# snd-review

Scrollable, zoomable multi-channel waveform viewer with region selection and
transport playback.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release
cargo run --release -- path/to/audio.wav
cargo run --release -- --list-devices
```

Supported formats include WAV, FLAC, MP3, OGG, and M4A (via Symphonia).

Open a file from the app with **File → Open…**, or drag and drop onto the
window. Press Space to play or pause.

## macOS app bundle

On macOS, package a double-clickable `.app` that can also be launched from the
terminal:

```bash
./script/bundle-macos
open target/release/snd-review.app
```

Install into `~/Applications` and put `snd-review` on your PATH (no sudo):

```bash
./script/bundle-macos --install
snd-review --help
snd-review path/to/audio.wav
```

That copies the bundle to `~/Applications/snd-review.app` and symlinks
`~/.local/bin/snd-review` to the binary inside it. If `~/.local/bin` is not
already on your PATH, add this to `~/.zshrc` and open a new terminal:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## License

MIT — see [LICENSE](LICENSE).
