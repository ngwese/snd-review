# Building snd-review

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) (2021 edition)

## Build

```bash
cargo build --release
```

```bash
cargo run --release
cargo run --release -- path/to/audio.wav
cargo run --release -- --list-devices
```

## App icon

The in-app mark, Windows `.exe` icon, and macOS `.app` icon all come from
[assets/logo/04-bands.svg](assets/logo/04-bands.svg) (logo study 4). Raster
assets live in [assets/app-icon/](assets/app-icon/):

| File | Used by |
| --- | --- |
| `app-icon.ico` | Embedded into the Windows executable at compile time |
| `AppIcon.icns` | Copied into `snd-review.app/Contents/Resources/` |
| `app-icon.png` | 512px PNG of the same artwork |

After changing the SVG, regenerate those files (requires
[resvg_py](https://pypi.org/project/resvg_py/)):

```bash
pip install resvg_py
python script/generate-app-icon.py
```

On Windows, `build.rs` embeds `app-icon.ico` into the binary. On macOS,
`script/bundle-macos` copies `AppIcon.icns` into the bundle. The title-bar
glyph (Windows and Linux only) loads the SVG directly.

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

## Windows executable

On Windows, package a double-clickable `.exe` that can also be launched from
the terminal:

```powershell
powershell -File script/bundle-windows.ps1
```

That writes `target\release\snd-review.exe`. Double-click it, or run it from
a console:

```powershell
.\target\release\snd-review.exe
.\target\release\snd-review.exe --help
.\target\release\snd-review.exe path\to\audio.wav
```

Install into `%LOCALAPPDATA%\snd-review`, put `snd-review` on your PATH, and
add a Start Menu shortcut (no admin):

```powershell
powershell -File script/bundle-windows.ps1 --install
snd-review --help
snd-review path\to\audio.wav
```

That copies the exe to `%LOCALAPPDATA%\snd-review\snd-review.exe`, links
`%USERPROFILE%\.local\bin\snd-review.exe` to it, and creates a Start Menu
shortcut. If `~\.local\bin` is not already on your PATH, add it in PowerShell
and open a new terminal:

```powershell
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable(
    "Path",
    "$userPath;$env:USERPROFILE\.local\bin",
    "User"
)
```
