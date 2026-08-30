use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::Parser;

mod app;
mod audio;
mod components;
mod model;
mod playback;

#[derive(Parser, Debug)]
#[command(
    name = "snd-review",
    about = "Display an audio file as a scrollable, zoomable multi-channel waveform",
    after_help = "Supports WAV, FLAC, MP3, OGG, M4A, and other formats enabled by Symphonia."
)]
struct Args {
    /// Path to an audio file
    file: Option<PathBuf>,

    /// List available output audio devices and exit
    #[arg(long)]
    list_devices: bool,

    /// Output device name or index (default: system default)
    #[arg(long, id = "output")]
    output_device: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.list_devices {
        playback::print_output_devices()?;
        return Ok(());
    }

    let file = args
        .file
        .as_ref()
        .context("audio file path is required unless --list-devices is used")?;
    let buffer = audio::load_buffer(file)
        .with_context(|| format!("failed to load {}", file.display()))?;
    let device = playback::resolve_output_device(args.output_device.as_deref())?;
    app::run(buffer, device);
    Ok(())
}
