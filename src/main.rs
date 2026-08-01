use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use legacy_android_screenshot::{CaptureOptions, PixelFormat, capture};

#[derive(Debug, Parser)]
#[command(
    name = "legacy-android-screenshot",
    version,
    about = "Capture a PNG screenshot from an older Android device over ADB"
)]
struct Cli {
    /// ADB serial number. Omit this when exactly one device is connected.
    #[arg(short = 's', long)]
    serial: Option<String>,

    /// Where to save the PNG.
    #[arg(short, long, default_value = "screenshot.png")]
    output: PathBuf,

    /// Linux framebuffer device on the phone.
    #[arg(long, default_value = "/dev/graphics/fb0")]
    framebuffer: String,

    /// Pixel layout used by the framebuffer. Auto is right for most devices.
    #[arg(long, value_enum, default_value_t = PixelFormatArg::Auto)]
    format: PixelFormatArg,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum PixelFormatArg {
    Auto,
    Rgba8888,
    Bgra8888,
    Rgb565,
    Bgr565,
    Rgb888,
}

impl From<PixelFormatArg> for PixelFormat {
    fn from(format: PixelFormatArg) -> Self {
        match format {
            PixelFormatArg::Auto => Self::Auto,
            PixelFormatArg::Rgba8888 => Self::Rgba8888,
            PixelFormatArg::Bgra8888 => Self::Bgra8888,
            PixelFormatArg::Rgb565 => Self::Rgb565,
            PixelFormatArg::Bgr565 => Self::Bgr565,
            PixelFormatArg::Rgb888 => Self::Rgb888,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let options = CaptureOptions {
        serial: cli.serial,
        output: cli.output,
        framebuffer: cli.framebuffer,
        pixel_format: cli.format.into(),
    };

    match capture(&options) {
        Ok(info) => println!(
            "Saved {}x{} screenshot from {} to {}",
            info.width,
            info.height,
            info.serial,
            info.output.display()
        ),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
