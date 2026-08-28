//! Capturing the screen for the QR scanner.
//!
//! The whole virtual desktop is captured in one shot and handed to the QR
//! decoder, which finds a code anywhere in it. That is deliberately simpler than
//! a drag-to-select overlay: there is no transparent always-on-top window to get
//! wrong, no interaction with the browser's own capture protection, and nothing
//! for a user to mis-drag. The captured pixels never leave the process.

use crate::{PlatformError, PlatformResult};

/// A captured screen image in RGBA8, top-down.
#[derive(Clone, PartialEq, Eq)]
pub struct ScreenCapture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for ScreenCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A screen capture may contain anything that was on screen. Never render
        // its pixels into a log line or a panic message.
        f.debug_struct("ScreenCapture")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.rgba.len())
            .finish()
    }
}

/// Largest capture accepted, in pixels per side.
#[cfg(windows)]
const MAX_DIMENSION: i32 = 32_767;

/// Captures the entire virtual desktop, spanning every monitor.
///
/// # Errors
///
/// Returns [`PlatformError::Api`] when any GDI call fails, and
/// [`PlatformError::Unsupported`] on non-Windows builds.
#[cfg(windows)]
pub fn capture_virtual_screen() -> PlatformResult<ScreenCapture> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    // SAFETY: `GetSystemMetrics` takes a plain enum value and returns an int. No
    // pointers, no ownership.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if width <= 0 || height <= 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PlatformError::Api {
            operation: "GetSystemMetrics",
            detail: format!("the virtual screen reported an unusable size of {width}x{height}"),
        });
    }

    // Each GDI resource below is released by the guard that owns it, in reverse
    // order of acquisition, on every exit path including the error paths.
    // SAFETY: `GetDC(None)` returns a device context for the entire screen, or a
    // null HDC on failure. Ownership is this thread's until `ReleaseDC`.
    let screen_dc = unsafe { GetDC(None) };
    if screen_dc.is_invalid() {
        return Err(PlatformError::Api {
            operation: "GetDC",
            detail: "the screen device context could not be obtained".to_owned(),
        });
    }
    // SAFETY: `screen_dc` is valid and owned here; `ReleaseDC` is the correct
    // release call for a DC obtained from `GetDC`, and runs exactly once.
    let _screen_guard = scopeguard(|| unsafe {
        ReleaseDC(None, screen_dc);
    });

    // SAFETY: `screen_dc` is a valid DC. The returned memory DC is owned by this
    // thread until `DeleteDC`.
    let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
    if memory_dc.is_invalid() {
        return Err(PlatformError::Api {
            operation: "CreateCompatibleDC",
            detail: "a memory device context could not be created".to_owned(),
        });
    }
    // SAFETY: `memory_dc` came from `CreateCompatibleDC` and is deleted exactly
    // once with the matching `DeleteDC`.
    let _memory_guard = scopeguard(|| unsafe {
        let _ = DeleteDC(memory_dc);
    });

    // SAFETY: `screen_dc` is valid and the dimensions are positive and bounded,
    // checked above. The returned bitmap is owned here until `DeleteObject`.
    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
    if bitmap.is_invalid() {
        return Err(PlatformError::Api {
            operation: "CreateCompatibleBitmap",
            detail: "a capture bitmap could not be created".to_owned(),
        });
    }
    // SAFETY: `bitmap` came from `CreateCompatibleBitmap` and is deleted exactly
    // once. It is deselected from `memory_dc` before deletion, below.
    let _bitmap_guard = scopeguard(|| unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    });

    // SAFETY: both handles are valid. `SelectObject` returns the previously
    // selected object, which is restored before `memory_dc` is deleted so the
    // bitmap is not still selected when it is deleted.
    let previous = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
    let _select_guard = scopeguard(|| unsafe {
        SelectObject(memory_dc, previous);
    });

    // SAFETY: both DCs are valid, the rectangle is inside the virtual screen by
    // construction, and CAPTUREBLT includes layered windows so a QR code shown
    // in one is captured too.
    unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            width,
            height,
            Some(screen_dc),
            left,
            top,
            SRCCOPY | CAPTUREBLT,
        )
    }
    .map_err(|e| PlatformError::Api {
        operation: "BitBlt",
        detail: e.to_string(),
    })?;

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or(0);
    info.bmiHeader.biWidth = width;
    // A negative height requests a top-down bitmap, which matches the row order
    // every image consumer in this workspace expects.
    info.bmiHeader.biHeight = -height;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let pixel_count = (width as usize) * (height as usize);
    let mut buffer = vec![0u8; pixel_count * 4];

    // SAFETY: `memory_dc` holds the captured bitmap. `buffer` is exactly
    // `width * height * 4` bytes, which is what a 32-bit BI_RGB DIB of these
    // dimensions requires, so `GetDIBits` cannot write out of bounds. `info` is
    // a live, fully initialised BITMAPINFO the callee reads and updates.
    let copied = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            u32::try_from(height).unwrap_or(0),
            Some(buffer.as_mut_ptr().cast()),
            &raw mut info,
            DIB_RGB_COLORS,
        )
    };
    if copied == 0 {
        return Err(PlatformError::Api {
            operation: "GetDIBits",
            detail: "the captured image could not be read back".to_owned(),
        });
    }

    // GDI hands back BGRA; every consumer here expects RGBA.
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        // A compatible bitmap leaves the alpha byte undefined. Force it opaque so
        // downstream decoders do not treat the image as fully transparent.
        pixel[3] = 0xff;
    }

    Ok(ScreenCapture {
        width: u32::try_from(width).unwrap_or(0),
        height: u32::try_from(height).unwrap_or(0),
        rgba: buffer,
    })
}

/// Runs `f` when the returned value is dropped.
///
/// A local three-line helper rather than a dependency: it exists only so each
/// GDI resource above is released on every exit path, including the `?` paths.
#[cfg(windows)]
fn scopeguard<F: FnMut()>(f: F) -> impl Drop {
    struct Guard<F: FnMut()>(F);
    impl<F: FnMut()> Drop for Guard<F> {
        fn drop(&mut self) {
            (self.0)();
        }
    }
    Guard(f)
}

/// Screen capture is Windows-only.
///
/// # Errors
///
/// Always returns [`PlatformError::Unsupported`].
#[cfg(not(windows))]
pub fn capture_virtual_screen() -> PlatformResult<ScreenCapture> {
    Err(PlatformError::Unsupported("screen capture"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capture_never_renders_its_pixels_in_debug_output() {
        let capture = ScreenCapture {
            width: 2,
            height: 1,
            rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let rendered = format!("{capture:?}");
        assert_eq!(rendered, "ScreenCapture { width: 2, height: 1, bytes: 8 }");
    }

    #[cfg(not(windows))]
    #[test]
    fn capture_is_reported_as_unavailable_off_windows() {
        assert!(matches!(
            capture_virtual_screen(),
            Err(PlatformError::Unsupported(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_capture_has_the_expected_buffer_length() {
        // Headless CI agents still have a virtual screen; if the call fails the
        // test asserts on the error type rather than the pixels.
        match capture_virtual_screen() {
            Ok(capture) => {
                assert!(capture.width > 0 && capture.height > 0);
                assert_eq!(
                    capture.rgba.len(),
                    (capture.width as usize) * (capture.height as usize) * 4
                );
            }
            Err(e) => assert!(matches!(e, PlatformError::Api { .. })),
        }
    }
}
