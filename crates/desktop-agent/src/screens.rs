//! On-demand capture, isolated from Host/session locks. No input/control operations.
use ai_terminal_protocol::{
    local::{Operation, Reply, Request},
    screens::{
        MAX_SCREEN_JPEG_BYTES, MAX_SCREEN_JSON_BYTES, MAX_SCREEN_WIDTH, MAX_SCREENS, ScreenFrame,
        ScreenInfo, ScreenList, valid_frame_request,
    },
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, GenericImageView, imageops::FilterType};
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(2);
// RGBA source <=64 MiB. Native libraries may hold an additional source copy;
// resize is capped at 1920x1920. Never allocate from network dimensions.
const MAX_SOURCE_PIXELS: u64 = 16 * 1024 * 1024;
static CAPTURE_BUSY: AtomicBool = AtomicBool::new(false);
struct Permit(&'static AtomicBool);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn bounded_job<T: Send + 'static>(
    busy: &'static AtomicBool,
    timeout: Duration,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    ensure!(
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok(),
        "screen_capture_busy: another screen request is pending"
    );
    let permit = Permit(busy);
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            // A native API cannot always be cancelled. Keep the permit after timeout until
            // it returns, preventing unbounded threads/allocations from repeated requests.
            let _permit = permit;
            let _ = tx.send(work());
        })
        .context("screen capture worker unavailable")?;
    rx.recv_timeout(timeout).map_err(|error| match error {
        mpsc::RecvTimeoutError::Timeout => {
            anyhow::anyhow!("screen_capture_timeout: screen capture did not respond")
        }
        mpsc::RecvTimeoutError::Disconnected => {
            anyhow::anyhow!("screen_capture_failed: capture worker stopped")
        }
    })?
}

pub(crate) fn dispatch(request: Request) -> Result<Reply> {
    dispatch_inner(request).map_err(preserve_rpc_error)
}
fn preserve_rpc_error(error: anyhow::Error) -> anyhow::Error {
    // service/Client serialize Display, which otherwise drops context's source chain.
    anyhow::anyhow!("{error:#}")
}
fn dispatch_inner(request: Request) -> Result<Reply> {
    let operation = Operation::try_from(request.operation)?;
    ensure!(
        request.session.is_empty(),
        "screen requests must not select a terminal"
    );
    if operation == Operation::RemoteScreenFrame {
        ensure!(
            valid_frame_request(&request.screen_id, request.screen_max_width),
            "invalid_screen_request"
        );
    }
    bounded_job(&CAPTURE_BUSY, CAPTURE_TIMEOUT, move || {
        let mut screens = native::enumerate()
            .context("screen_enumeration_failed: Desktop screen access unavailable")?;
        ensure!(screens.len() <= MAX_SCREENS, "screen_list_limit");
        let json = match operation {
            Operation::RemoteScreens => serde_json::to_string(&ScreenList {
                screens: screens.into_iter().map(|screen| screen.info).collect(),
            })?,
            Operation::RemoteScreenFrame => {
                let index = screen_index(
                    screens.iter().map(|screen| &screen.info),
                    &request.screen_id,
                )?;
                let screen = screens.swap_remove(index);
                check_source_size(screen.info.width, screen.info.height)?;
                let capture = native::capture(&screen, request.screen_max_width).context(
                    "screen_capture_failed: allow screen recording/capture on Desktop and retry",
                )?;
                let captured_at_ms =
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
                let (bytes, width, height) = encode_preview(capture, request.screen_max_width)?;
                serde_json::to_string(&ScreenFrame {
                    screen_id: screen.info.id,
                    mime_type: "image/jpeg".into(),
                    image_base64: STANDARD.encode(bytes),
                    width,
                    height,
                    captured_at_ms,
                })?
            }
            _ => bail!("unsupported screen operation"),
        };
        ensure!(json.len() <= MAX_SCREEN_JSON_BYTES, "screen_frame_limit");
        Ok(Reply {
            history: vec![json],
            ..Default::default()
        })
    })
}

fn screen_index<'a>(screens: impl Iterator<Item = &'a ScreenInfo>, id: &str) -> Result<usize> {
    screens
        .enumerate()
        .find_map(|(index, screen)| (screen.id == id).then_some(index))
        .context("unknown_screen: screen disconnected or identity changed")
}

fn check_source_size(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS,
        "screen_size_limit: Desktop screen exceeds capture memory limit"
    );
    Ok(())
}
struct LimitedBytes(Vec<u8>);
impl Write for LimitedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_SCREEN_JPEG_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("screen_frame_limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode_preview(capture: DynamicImage, max_width: u32) -> Result<(Vec<u8>, u32, u32)> {
    let (width, height) = capture.dimensions();
    check_source_size(width, height)?;
    let mut preview = capture
        .resize(
            max_width.min(width),
            MAX_SCREEN_WIDTH.min(height),
            FilterType::Triangle,
        )
        .to_rgb8();
    // Budget noisy/high-entropy desktops by reducing quality, then dimensions.
    loop {
        for quality in [75, 55, 35] {
            let mut out = LimitedBytes(Vec::new());
            if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
                .encode_image(&preview)
                .is_ok()
            {
                return Ok((out.0, preview.width(), preview.height()));
            }
        }
        ensure!(
            preview.width() > 64 && preview.height() > 64,
            "screen_frame_limit"
        );
        preview = image::imageops::resize(
            &preview,
            (preview.width() * 3 / 4).max(1),
            (preview.height() * 3 / 4).max(1),
            FilterType::Triangle,
        );
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use core_graphics::display::CGDisplay;
    use std::process::{Command, Stdio};
    pub(super) struct Screen {
        pub info: ScreenInfo,
        display: CGDisplay,
    }
    pub(super) fn enumerate() -> Result<Vec<Screen>> {
        CGDisplay::active_displays()
            .map_err(|code| anyhow::anyhow!("CoreGraphics enumeration error {code}"))?
            .into_iter()
            .map(|id| {
                let display = CGDisplay::new(id);
                let mode = display
                    .display_mode()
                    .context("screen display mode unavailable")?;
                let (width, height) = (
                    u32::try_from(mode.pixel_width())?,
                    u32::try_from(mode.pixel_height())?,
                );
                let name = if display.is_builtin() {
                    "Built-in display".to_string()
                } else {
                    format!("Display {}", display.id)
                };
                Ok(Screen {
                    info: ScreenInfo {
                        id: format!("macos:{id}"),
                        name,
                        width,
                        height,
                        is_primary: display.is_main(),
                    },
                    display,
                })
            })
            .collect()
    }
    pub(super) fn capture(screen: &Screen, max_width: u32) -> Result<DynamicImage> {
        ensure!(
            core_graphics::access::ScreenCaptureAccess.preflight(),
            "screen_capture_permission_denied: enable Screen Recording for Desktop in System Settings"
        );
        let temp = tempfile::tempdir()?; // Private 0700 directory, removed on every exit.
        let path = temp.path().join("capture.jpg");
        let rect = screen.display.bounds();
        let region = format!(
            "{},{},{},{}",
            rect.origin.x as i32,
            rect.origin.y as i32,
            rect.size.width as u32,
            rect.size.height as u32
        );
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        run_command(
            Command::new("/usr/sbin/screencapture")
                .args(["-x", "-R", &region, "-t", "jpg"])
                .arg(&path),
            deadline,
        )?;
        ensure!(
            std::fs::metadata(&path)?.len() <= 32 * 1024 * 1024,
            "screen_size_limit"
        );
        // Downsample through the system image service before Rust decoding. This keeps
        // large Retina displays responsive even in unoptimized Desktop development builds.
        run_command(
            Command::new("/usr/bin/sips")
                .args(["-Z", &max_width.to_string()])
                .arg(&path),
            deadline,
        )?;
        let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(MAX_SOURCE_PIXELS * 4);
        reader.limits(limits);
        Ok(reader.decode()?)
    }
    fn run_command(command: &mut Command, deadline: std::time::Instant) -> Result<()> {
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    ensure!(
                        status.success(),
                        "screen_capture_failed: allow Screen Recording in Desktop System Settings"
                    );
                    return Ok(());
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error.into());
                }
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("screen_capture_timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod native {
    use super::*;
    pub(super) struct Screen {
        pub info: ScreenInfo,
        monitor: xcap::Monitor,
    }
    pub(super) fn enumerate() -> Result<Vec<Screen>> {
        #[cfg(target_os = "linux")]
        ensure!(
            !std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
                && std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "screen_capture_unsupported: Wayland screen capture is unavailable; use an X11 Desktop session"
        );
        #[cfg(target_os = "linux")]
        let pixel_sizes = {
            use xcb::Xid;
            let (conn, index) = xcb::Connection::connect(None)?;
            let root = conn
                .get_setup()
                .roots()
                .nth(index as usize)
                .context("X11 screen unavailable")?
                .root();
            let reply = conn.wait_for_reply(conn.send_request(&xcb::randr::GetMonitors {
                window: root,
                get_active: true,
            }))?;
            let mut sizes = std::collections::HashMap::new();
            for monitor in reply.monitors() {
                for output in monitor.outputs() {
                    sizes.insert(
                        output.resource_id(),
                        (u32::from(monitor.width()), u32::from(monitor.height())),
                    );
                }
            }
            sizes
        };
        xcap::Monitor::all()?
            .into_iter()
            .map(|monitor| {
                let id = format!("{}:{}", std::env::consts::OS, monitor.id()?);
                #[cfg(target_os = "windows")]
                let (width, height) = (monitor.width()?, monitor.height()?);
                #[cfg(target_os = "linux")]
                let (width, height) = *pixel_sizes
                    .get(&monitor.id()?)
                    .context("screen disconnected during enumeration")?;
                let name = monitor.name()?;
                ensure!(
                    name.len() <= 256 && width > 0 && height > 0,
                    "invalid screen metadata"
                );
                Ok(Screen {
                    info: ScreenInfo {
                        id,
                        name,
                        width,
                        height,
                        is_primary: monitor.is_primary()?,
                    },
                    monitor,
                })
            })
            .collect()
    }
    pub(super) fn capture(screen: &Screen, _: u32) -> Result<DynamicImage> {
        Ok(DynamicImage::ImageRgba8(screen.monitor.capture_image()?))
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod native {
    use super::*;
    pub(super) struct Screen {
        pub info: ScreenInfo,
    }
    pub(super) fn enumerate() -> Result<Vec<Screen>> {
        bail!("screen_capture_unsupported: unsupported Desktop platform")
    }
    pub(super) fn capture(_: &Screen, _: u32) -> Result<DynamicImage> {
        bail!("screen_capture_unsupported")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_BUSY: AtomicBool = AtomicBool::new(false);
    #[test]
    fn timed_out_capture_keeps_the_single_worker_permit_until_native_call_returns() {
        let (release, wait) = mpsc::channel();
        assert!(
            bounded_job(&TEST_BUSY, Duration::from_millis(20), move || {
                wait.recv().unwrap();
                Ok(())
            })
            .unwrap_err()
            .to_string()
            .contains("timeout")
        );
        assert!(
            bounded_job(&TEST_BUSY, Duration::from_millis(20), || Ok(()))
                .unwrap_err()
                .to_string()
                .contains("busy")
        );
        release.send(()).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(1);
        while TEST_BUSY.load(Ordering::Acquire) && std::time::Instant::now() < until {
            std::thread::yield_now();
        }
        assert!(!TEST_BUSY.load(Ordering::Acquire));
    }
    #[test]
    fn noisy_and_portrait_frames_fit_real_transport_budget_and_decode_as_jpeg() {
        for (width, height, max_width) in [(1920, 1080, 1920), (100, 2000, 90), (10, 10, 100)] {
            let mut random = 1u32;
            let capture = image::RgbImage::from_fn(width, height, |_, _| {
                let mut rgb = [0; 3];
                for value in &mut rgb {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    *value = (random >> 24) as u8;
                }
                image::Rgb(rgb)
            });
            let (bytes, w, h) =
                encode_preview(DynamicImage::ImageRgb8(capture), max_width).unwrap();
            assert!(bytes.len() <= MAX_SCREEN_JPEG_BYTES);
            assert!(w <= width.min(max_width) && h <= height.min(MAX_SCREEN_WIDTH));
            assert_eq!(
                image::load_from_memory(&bytes).unwrap().dimensions(),
                (w, h)
            );
        }
        assert!(check_source_size(0, 1).is_err());
        assert!(check_source_size(u32::MAX, u32::MAX).is_err());
    }
    #[test]
    fn unknown_and_disconnected_screens_are_not_replaced_by_primary_display() {
        let screens = [ScreenInfo {
            id: "native:2".into(),
            name: "Display".into(),
            width: 1920,
            height: 1080,
            is_primary: true,
        }];
        assert_eq!(screen_index(screens.iter(), "native:2").unwrap(), 0);
        assert!(
            screen_index(screens.iter(), "native:1")
                .unwrap_err()
                .to_string()
                .contains("unknown_screen")
        );
        assert!(screen_index(std::iter::empty(), "native:2").is_err());
    }
    #[test]
    #[ignore = "requires a real Desktop and previously granted screen recording permission"]
    fn real_desktop_frame_capture() {
        let screen = native::enumerate()
            .unwrap()
            .into_iter()
            .next()
            .expect("real display");
        let reply = dispatch(Request {
            operation: Operation::RemoteScreenFrame as i32,
            screen_id: screen.info.id,
            screen_max_width: 1200,
            ..Default::default()
        })
        .unwrap();
        let frame: ScreenFrame = serde_json::from_str(&reply.history[0]).unwrap();
        let bytes = STANDARD.decode(frame.image_base64).unwrap();
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (frame.width, frame.height)
        );
        println!(
            "Captured JPEG {}x{}, {} bytes",
            frame.width,
            frame.height,
            bytes.len()
        );
    }
    #[test]
    fn screen_error_codes_survive_context_and_rpc_display_serialization() {
        use prost::Message;
        for code in [
            "screen_capture_unsupported: Wayland",
            "screen_capture_permission_denied",
            "screen_capture_timeout",
            "unknown_screen",
        ] {
            let error = anyhow::anyhow!("{code}")
                .context("native backend failed")
                .context("screen_enumeration_failed: Desktop screen access unavailable");
            let reply = Reply {
                error: preserve_rpc_error(error).to_string(),
                ..Default::default()
            };
            let received = Reply::decode(reply.encode_to_vec().as_slice()).unwrap();
            let client_error = anyhow::anyhow!("{}", received.error);
            assert!(client_error.to_string().contains(code));
            assert!(client_error.to_string().contains("native backend failed"));
        }
    }
    #[test]
    fn invalid_frame_requests_fail_before_enumeration() {
        for (screen_id, screen_max_width) in [("", 1), ("id", 0), ("id", MAX_SCREEN_WIDTH + 1)] {
            let result = dispatch(Request {
                operation: Operation::RemoteScreenFrame as i32,
                screen_id: screen_id.into(),
                screen_max_width,
                ..Default::default()
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("invalid_screen_request")
            );
        }
    }
    #[test]
    #[ignore = "requires a real Desktop session; enumerates metadata only, never captures pixels"]
    fn real_desktop_screen_metadata() {
        let screens = native::enumerate().unwrap();
        println!(
            "{}",
            serde_json::to_string(&ScreenList {
                screens: screens.into_iter().map(|screen| screen.info).collect()
            })
            .unwrap()
        );
    }
}
