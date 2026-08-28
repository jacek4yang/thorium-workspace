//! Bringing an already-running browser window to the front.
//!
//! Launching a profile that is already running must show the existing session
//! rather than start a conflicting second one against the same `User Data`
//! directory.

use crate::PlatformResult;

/// Activates a visible top-level window belonging to `pid`.
///
/// Returns `true` when a window was found and activation was attempted. Windows
/// may still refuse to change the foreground window when the calling process
/// does not have focus; in that case the taskbar button flashes instead, which
/// is the documented behaviour and is what a user expects.
///
/// # Errors
///
/// Never fails: an inability to find or raise a window is reported as `false` so
/// the caller can fall back to telling the user the profile is already running.
#[cfg(windows)]
pub fn focus_window_of_process(pid: u32) -> PlatformResult<bool> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows::core::BOOL;

    /// Passed to `EnumWindows` as the user parameter.
    struct Search {
        pid: u32,
        found: Option<HWND>,
    }

    /// Called by `EnumWindows` once per top-level window.
    ///
    /// # Safety
    ///
    /// `lparam` must be a `*mut Search` that outlives the enumeration. Only
    /// `focus_window_of_process` installs this callback, and it passes a pointer
    /// to a stack local that is alive for the whole `EnumWindows` call.
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: the caller contract above guarantees `lparam` is a valid,
        // uniquely-borrowed `*mut Search` for the duration of the enumeration.
        // `EnumWindows` is single-threaded and synchronous, so no other
        // reference to the same `Search` exists while this runs.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut window_pid: u32 = 0;
        // SAFETY: `hwnd` is supplied by the enumerator and is valid for the
        // duration of this callback; `window_pid` is a live stack local the
        // callee writes exactly one u32 into.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut window_pid)) };
        // SAFETY: `hwnd` is valid for the duration of this callback.
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if window_pid == search.pid && visible {
            search.found = Some(hwnd);
            // Returning FALSE stops the enumeration; the first visible window
            // belonging to the process is the one to raise.
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut search = Search { pid, found: None };
    // SAFETY: `visit` matches the `WNDENUMPROC` signature, and the pointer
    // passed as `lparam` refers to `search`, a stack local that outlives this
    // synchronous call. `EnumWindows` returning an error only means the
    // enumeration was stopped early, which is exactly what `visit` does on a
    // match, so the result is deliberately not treated as a failure.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(std::ptr::from_mut(&mut search) as isize)) };

    let Some(hwnd) = search.found else {
        return Ok(false);
    };

    // SAFETY: `hwnd` was produced by the enumeration above. A window can close
    // between enumeration and here; both calls tolerate a stale handle by
    // returning an error or FALSE rather than by any unsafe behaviour.
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(true)
}

/// Activating a window is Windows-only; other builds report that no window was
/// raised so the caller falls back to a message.
///
/// # Errors
///
/// Never fails.
#[cfg(not(windows))]
pub fn focus_window_of_process(_pid: u32) -> PlatformResult<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focusing_an_unknown_process_reports_that_nothing_was_raised() {
        assert!(!focus_window_of_process(u32::MAX - 1).expect("no error"));
    }
}
