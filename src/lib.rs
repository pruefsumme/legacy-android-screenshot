use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PixelFormat {
    Auto,
    Rgba8888,
    Bgra8888,
    Rgb565,
    Bgr565,
    Rgb888,
}

#[derive(Debug)]
pub struct CaptureOptions {
    pub serial: Option<String>,
    pub output: PathBuf,
    pub framebuffer: String,
    pub pixel_format: PixelFormat,
}

#[derive(Debug)]
pub struct CaptureInfo {
    pub serial: String,
    pub width: u32,
    pub height: u32,
    pub bits_per_pixel: u32,
    pub output: PathBuf,
}

#[derive(Debug)]
pub enum Error {
    AdbNotFound,
    CommandFailed { command: String, details: String },
    InvalidDeviceList(String),
    InvalidFramebuffer(String),
    Io(io::Error),
    Png(png::EncodingError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdbNotFound => write!(f, "adb was not found on PATH"),
            Self::CommandFailed { command, details } => {
                write!(f, "command failed: {command}")?;
                if !details.is_empty() {
                    write!(f, "\n{details}")?;
                }
                Ok(())
            }
            Self::InvalidDeviceList(message) => write!(f, "{message}"),
            Self::InvalidFramebuffer(message) => write!(f, "{message}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::Png(error) => write!(f, "could not write PNG: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<png::EncodingError> for Error {
    fn from(error: png::EncodingError) -> Self {
        Self::Png(error)
    }
}

struct Adb {
    serial: String,
}

struct FramebufferInfo {
    width: u32,
    height: u32,
    bits_per_pixel: u32,
    stride: usize,
}

pub fn capture(options: &CaptureOptions) -> Result<CaptureInfo, Error> {
    let serial = match &options.serial {
        Some(serial) => serial.clone(),
        None => find_single_device()?,
    };
    let adb = Adb {
        serial: serial.clone(),
    };
    let framebuffer = FramebufferInfo::read(&adb, &options.framebuffer)?;
    let remote_path = temporary_remote_path();
    let local_path = temporary_local_path();

    let result = capture_frame(
        &adb,
        &framebuffer,
        &options.framebuffer,
        &remote_path,
        &local_path,
        options,
    );

    let _ = adb.shell(&["rm", &remote_path]);
    let _ = fs::remove_file(&local_path);
    result.map(|()| CaptureInfo {
        serial,
        width: framebuffer.width,
        height: framebuffer.height,
        bits_per_pixel: framebuffer.bits_per_pixel,
        output: options.output.clone(),
    })
}

fn capture_frame(
    adb: &Adb,
    framebuffer: &FramebufferInfo,
    framebuffer_path: &str,
    remote_path: &str,
    local_path: &Path,
    options: &CaptureOptions,
) -> Result<(), Error> {
    let frame_bytes = framebuffer
        .stride
        .checked_mul(framebuffer.height as usize)
        .ok_or_else(|| Error::InvalidFramebuffer("framebuffer size is too large".into()))?;

    adb.shell(&[
        "dd",
        &format!("if={framebuffer_path}"),
        &format!("of={remote_path}"),
        &format!("bs={frame_bytes}"),
        "count=1",
    ])?;
    adb.pull(remote_path, local_path)?;

    let mut raw = Vec::with_capacity(frame_bytes);
    File::open(local_path)?.read_to_end(&mut raw)?;
    if raw.len() < frame_bytes {
        return Err(Error::InvalidFramebuffer(format!(
            "device returned {} bytes, expected {frame_bytes}",
            raw.len()
        )));
    }

    let format = match options.pixel_format {
        PixelFormat::Auto => match framebuffer.bits_per_pixel {
            16 => PixelFormat::Rgb565,
            24 => PixelFormat::Rgb888,
            32 => PixelFormat::Rgba8888,
            bits => {
                return Err(Error::InvalidFramebuffer(format!(
                    "unsupported framebuffer depth: {bits} bits per pixel"
                )));
            }
        },
        format => format,
    };
    let rgba = decode_pixels(
        &raw[..frame_bytes],
        framebuffer.width,
        framebuffer.height,
        framebuffer.stride,
        format,
    )?;
    write_png(
        &options.output,
        framebuffer.width,
        framebuffer.height,
        &rgba,
    )?;
    Ok(())
}

impl Adb {
    fn command(&self, args: &[&str]) -> Result<Output, Error> {
        let mut command = Command::new("adb");
        command.arg("-s").arg(&self.serial).args(args);
        command.output().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Error::AdbNotFound
            } else {
                Error::Io(error)
            }
        })
    }

    fn shell(&self, args: &[&str]) -> Result<String, Error> {
        let mut command_args = vec!["shell"];
        command_args.extend_from_slice(args);
        let output = self.command(&command_args)?;
        checked_output(&command_args, output)
    }

    fn pull(&self, remote: &str, local: &Path) -> Result<(), Error> {
        let local = local.to_string_lossy().into_owned();
        let args = ["pull", remote, &local];
        let output = self.command(&args)?;
        checked_output(&args, output).map(|_| ())
    }
}

fn checked_output(args: &[&str], output: Output) -> Result<String, Error> {
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(Error::CommandFailed {
            command: format!("adb {}", args.join(" ")),
            details,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn find_single_device() -> Result<String, Error> {
    let output = Command::new("adb")
        .args(["devices"])
        .output()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                Error::AdbNotFound
            } else {
                Error::Io(error)
            }
        })?;
    let text = checked_output(&["devices"], output)?;
    let devices: Vec<&str> = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            let state = fields.next()?;
            (state == "device").then_some(serial)
        })
        .collect();

    match devices.as_slice() {
        [serial] => Ok((*serial).to_string()),
        [] => Err(Error::InvalidDeviceList(
            "no usable Android device was found; connect one and enable USB debugging".into(),
        )),
        _ => Err(Error::InvalidDeviceList(format!(
            "more than one Android device is connected; use --serial (available: {})",
            devices.join(", ")
        ))),
    }
}

impl FramebufferInfo {
    fn read(adb: &Adb, framebuffer_path: &str) -> Result<Self, Error> {
        let name = Path::new(framebuffer_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("fb0");
        let base = format!("/sys/class/graphics/{name}");
        let virtual_size = read_required(adb, &format!("{base}/virtual_size"))?;
        let mut dimensions = virtual_size.split(',');
        let width = parse_number(dimensions.next(), "framebuffer width")?;
        let virtual_height = parse_number(dimensions.next(), "framebuffer height")?;
        let bits_per_pixel = parse_number(
            Some(&read_required(adb, &format!("{base}/bits_per_pixel"))?),
            "bits per pixel",
        )?;
        let height = read_mode_height(adb, &base).unwrap_or_else(|| {
            if virtual_height >= width && virtual_height % 2 == 0 {
                virtual_height / 2
            } else {
                virtual_height
            }
        });
        let default_stride = (width as usize)
            .saturating_mul(bits_per_pixel as usize)
            .div_ceil(8);
        let stride = read_optional(adb, &format!("{base}/stride"))
            .and_then(|value| value.parse().ok())
            .unwrap_or(default_stride);

        if width == 0 || height == 0 || stride < default_stride {
            return Err(Error::InvalidFramebuffer(format!(
                "invalid framebuffer geometry: {width}x{height}, stride {stride}"
            )));
        }
        Ok(Self {
            width,
            height,
            bits_per_pixel,
            stride,
        })
    }
}

fn read_required(adb: &Adb, path: &str) -> Result<String, Error> {
    Ok(adb.shell(&["cat", path])?.trim().to_string())
}

fn read_optional(adb: &Adb, path: &str) -> Option<String> {
    adb.shell(&["cat", path])
        .ok()
        .map(|value| value.trim().to_string())
}

fn read_mode_height(adb: &Adb, base: &str) -> Option<u32> {
    let modes = read_optional(adb, &format!("{base}/modes"))?;
    modes.lines().find_map(parse_mode_height)
}

fn parse_mode_height(line: &str) -> Option<u32> {
    let marker = line.find('x')?;
    let after_x = &line[marker + 1..];
    let digits: String = after_x.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn parse_number(value: Option<&str>, label: &str) -> Result<u32, Error> {
    value
        .ok_or_else(|| Error::InvalidFramebuffer(format!("missing {label}")))?
        .trim()
        .parse()
        .map_err(|_| Error::InvalidFramebuffer(format!("invalid {label}")))
}

fn temporary_remote_path() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!(
        "/data/local/tmp/legacy-android-screenshot-{}-{timestamp}.raw",
        std::process::id()
    )
}

fn temporary_local_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "legacy-android-screenshot-{}.raw",
        std::process::id()
    ))
}

fn decode_pixels(
    raw: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    format: PixelFormat,
) -> Result<Vec<u8>, Error> {
    let bytes_per_pixel = match format {
        PixelFormat::Rgba8888 | PixelFormat::Bgra8888 => 4,
        PixelFormat::Rgb565 | PixelFormat::Bgr565 => 2,
        PixelFormat::Rgb888 => 3,
        PixelFormat::Auto => unreachable!(),
    };
    let row_bytes = width as usize * bytes_per_pixel;
    let required = stride
        .checked_mul(height as usize)
        .ok_or_else(|| Error::InvalidFramebuffer("framebuffer size is too large".into()))?;
    if stride < row_bytes || raw.len() < required {
        return Err(Error::InvalidFramebuffer(
            "framebuffer data is truncated".into(),
        ));
    }

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in raw[..required].chunks_exact(stride).take(height as usize) {
        for pixel in row[..row_bytes].chunks_exact(bytes_per_pixel) {
            let (red, green, blue, alpha) = match format {
                PixelFormat::Rgba8888 => (pixel[0], pixel[1], pixel[2], pixel[3]),
                PixelFormat::Bgra8888 => (pixel[2], pixel[1], pixel[0], pixel[3]),
                PixelFormat::Rgb888 => (pixel[0], pixel[1], pixel[2], 255),
                PixelFormat::Rgb565 | PixelFormat::Bgr565 => {
                    let value = u16::from_le_bytes([pixel[0], pixel[1]]);
                    let (red, blue) = if format == PixelFormat::Rgb565 {
                        (value >> 11, value & 0x1f)
                    } else {
                        (value & 0x1f, value >> 11)
                    };
                    (
                        expand_5(red as u8),
                        expand_6(((value >> 5) & 0x3f) as u8),
                        expand_5(blue as u8),
                        255,
                    )
                }
                PixelFormat::Auto => unreachable!(),
            };
            rgba.extend_from_slice(&[red, green, blue, alpha]);
        }
    }
    Ok(rgba)
}

fn expand_5(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

fn expand_6(value: u8) -> u8 {
    (value << 2) | (value >> 4)
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_height() {
        assert_eq!(parse_mode_height("U:480x800p-0"), Some(800));
    }

    #[test]
    fn decodes_rgb565() {
        let white = decode_pixels(&[0xff, 0xff], 1, 1, 2, PixelFormat::Rgb565).unwrap();
        assert_eq!(white, [255, 255, 255, 255]);
    }

    #[test]
    fn respects_stride() {
        let rgba = decode_pixels(
            &[255, 0, 0, 255, 0, 0, 0, 0],
            1,
            1,
            8,
            PixelFormat::Rgba8888,
        )
        .unwrap();
        assert_eq!(rgba, [255, 0, 0, 255]);
    }
}
