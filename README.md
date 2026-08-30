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

## License

MIT — see [LICENSE](LICENSE).
