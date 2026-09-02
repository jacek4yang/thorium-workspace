//! UTF-16 conversion helpers for Win32 wide-string parameters.

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

/// Converts a path to a NUL-terminated UTF-16 buffer for `*PCWSTR`
/// parameters. Returns a typed error instead of panicking when the path
/// contains invalid surrogate data.
pub fn path_to_wide(path: &Path) -> Result<Vec<u16>, crate::error::PlatformError> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(crate::error::PlatformError::WideConversion {
            path: path.to_path_buf(),
        });
    }
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_terminator_and_preserves_content() {
        let path = Path::new("C:\\Workspace\\Profiles");
        let wide = path_to_wide(path).expect("convertible");
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(wide.len(), path.as_os_str().len() + 1);
    }

    #[test]
    fn unicode_paths_convert() {
        use std::os::windows::ffi::OsStrExt as _;
        let path = Path::new("D:\\Dane\\Zarządzanie profilami");
        let wide = path_to_wide(path).expect("convertible");
        // One UTF-16 unit per wide char, plus the NUL terminator.
        let expected = path.as_os_str().encode_wide().count() + 1;
        assert_eq!(wide.len(), expected);
        assert_eq!(wide.last(), Some(&0));
    }
}
