use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

    /// Print ADB commands, bytes, and decoding details to stderr.
    #[arg(short = 'v', long)]
    verbose: bool,
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
    let loader = (!cli.verbose && io::stderr().is_terminal()).then(Loader::start);
    let options = CaptureOptions {
        serial: cli.serial,
        output: cli.output,
        framebuffer: cli.framebuffer,
        pixel_format: cli.format.into(),
        verbose: cli.verbose,
    };

    let result = capture(&options);
    if let Some(loader) = loader {
        loader.finish();
    }

    match result {
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

struct Loader {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Loader {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let colors = [31, 33, 32, 36, 34, 35];
            let mut offset = 0;
            while !thread_stop.load(Ordering::Relaxed) {
                let mut output = String::from("\r  ");
                for dot in 0..4 {
                    output.push_str(&format!(
                        "\x1b[{}m.\x1b[0m ",
                        colors[(offset + dot) % colors.len()]
                    ));
                }
                output.push_str("capturing");
                print_stderr(&output);
                offset = (offset + 1) % colors.len();
                thread::sleep(Duration::from_millis(120));
            }
        });
        Self { stop, thread }
    }

    fn finish(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
        print_stderr("\r\x1b[2K");
    }
}

fn print_stderr(message: &str) {
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "{message}");
    let _ = stderr.flush();
}
