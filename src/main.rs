use std::{path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use clap::Parser;

mod app;
mod audio;

#[derive(Parser, Debug)]
#[command(
    name = "snd-review",
    about = "Display an audio file as a scrollable, zoomable multi-channel waveform",
    after_help = "Supports WAV, FLAC, MP3, OGG, M4A, and other formats enabled by Symphonia."
)]
struct Args {
    /// Path to an audio file
    file: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let decoded = audio::decode(&args.file)
        .with_context(|| format!("failed to decode {}", args.file.display()))?;
    app::run(Arc::new(decoded), args.file);
    Ok(())
}
