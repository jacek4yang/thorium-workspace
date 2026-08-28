//! QR code import for two-factor credentials.
//!
//! Decodes a QR code from an image file or from raw pixels (a clipboard image or
//! a screen capture) and turns the payload into an [`OtpCredential`].
//!
//! # Handling of decoded payloads
//!
//! A two-factor QR code *is* the shared secret. Nothing in this crate logs,
//! prints or returns a decoded payload except as the parsed credential, and no
//! error variant embeds any part of it. A payload that turns out not to be an
//! `otpauth://` URI is discarded without being reported back, because the user
//! may well have scanned something else sensitive by mistake.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::Path;

use image::{DynamicImage, GrayImage, ImageReader, Limits};
use tw_domain::DiagnosticCode;
use tw_otp::{OtpCredential, OtpUriError, parse_otpauth_uri};

/// Largest image accepted, in pixels per side.
///
/// Screenshots of 8K displays fit comfortably; anything larger is far more
/// likely to be a decompression bomb than a QR code.
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;

/// Largest allocation an image decode may make, in bytes.
pub const MAX_IMAGE_ALLOCATION: u64 = 256 * 1024 * 1024;

/// Largest image file accepted, in bytes.
pub const MAX_IMAGE_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Why a QR import failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QrError {
    /// The file could not be read.
    #[error("the image file could not be read")]
    FileUnreadable,
    /// The file is larger than [`MAX_IMAGE_FILE_BYTES`].
    #[error("the image file is too large to scan")]
    FileTooLarge,
    /// The bytes are not an image in a supported format.
    #[error("that file is not an image this app can read")]
    NotAnImage,
    /// The image exceeds the configured decode limits.
    #[error("the image is too large to scan safely")]
    ImageTooLarge,
    /// The pixel buffer's length does not match its stated dimensions.
    #[error("the image data is inconsistent with its dimensions")]
    MalformedPixels,
    /// No QR code was found.
    #[error("no QR code was found in that image")]
    NoQrCode,
    /// A QR code was found but its contents are not a two-factor URI.
    ///
    /// Deliberately says nothing about what was actually in it.
    #[error("that QR code is not a two-factor setup code")]
    NotAnOtpauthUri,
    /// A QR code held an `otpauth://` URI that could not be used.
    #[error("{0}")]
    UnusableCredential(#[from] OtpUriError),
}

impl QrError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::FileUnreadable | Self::FileTooLarge => DiagnosticCode::IoFailed,
            Self::NotAnImage | Self::ImageTooLarge | Self::MalformedPixels | Self::NoQrCode => {
                DiagnosticCode::QrNotFound
            }
            Self::NotAnOtpauthUri => DiagnosticCode::QrPayloadNotOtpauth,
            Self::UnusableCredential(_) => DiagnosticCode::OtpUriInvalid,
        }
    }
}

/// QR result alias.
pub type QrResult<T> = Result<T, QrError>;

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOCATION);
    limits
}

/// Reads an image file and returns the two-factor credential it encodes.
///
/// # Errors
///
/// See [`QrError`].
pub fn credential_from_image_file(path: &Path) -> QrResult<OtpCredential> {
    let metadata = std::fs::metadata(path).map_err(|_| QrError::FileUnreadable)?;
    if metadata.len() > MAX_IMAGE_FILE_BYTES {
        return Err(QrError::FileTooLarge);
    }
    let bytes = std::fs::read(path).map_err(|_| QrError::FileUnreadable)?;
    credential_from_image_bytes(&bytes)
}

/// Decodes an encoded image (PNG, JPEG, BMP, GIF, WebP) and returns the
/// two-factor credential it encodes.
///
/// # Errors
///
/// See [`QrError`].
pub fn credential_from_image_bytes(bytes: &[u8]) -> QrResult<OtpCredential> {
    if bytes.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(QrError::FileTooLarge);
    }
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| QrError::NotAnImage)?;
    if reader.format().is_none() {
        return Err(QrError::NotAnImage);
    }
    reader.limits(limits());
    let image = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => QrError::ImageTooLarge,
        _ => QrError::NotAnImage,
    })?;
    credential_from_image(&image)
}

/// Decodes a raw RGBA8 buffer, the form a Windows clipboard image arrives in.
///
/// # Errors
///
/// See [`QrError`].
pub fn credential_from_rgba(width: u32, height: u32, rgba: &[u8]) -> QrResult<OtpCredential> {
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(QrError::ImageTooLarge);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(QrError::ImageTooLarge)?;
    if rgba.len() != expected {
        return Err(QrError::MalformedPixels);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or(QrError::MalformedPixels)?;
    credential_from_image(&DynamicImage::ImageRgba8(buffer))
}

/// Decodes an already-loaded image.
///
/// # Errors
///
/// See [`QrError`].
pub fn credential_from_image(image: &DynamicImage) -> QrResult<OtpCredential> {
    let payloads = scan_payloads(&image.to_luma8());
    if payloads.is_empty() {
        return Err(QrError::NoQrCode);
    }
    let mut first_uri_error: Option<OtpUriError> = None;
    for payload in &payloads {
        match parse_otpauth_uri(payload) {
            Ok(credential) => return Ok(credential),
            // Not an otpauth URI at all: keep looking, and never report what it
            // actually was.
            Err(OtpUriError::NotAUri | OtpUriError::WrongScheme) => {}
            // A malformed otpauth URI is worth reporting, but only after every
            // other code in the image has been tried.
            Err(other) => {
                first_uri_error.get_or_insert(other);
            }
        }
    }
    match first_uri_error {
        Some(err) => Err(QrError::UnusableCredential(err)),
        None => Err(QrError::NotAnOtpauthUri),
    }
}

/// Returns every QR payload found in a greyscale image.
///
/// Screen captures often contain more than one QR code; the caller decides which
/// payloads matter. Kept private so a raw payload cannot escape this crate.
fn scan_payloads(image: &GrayImage) -> Vec<String> {
    let mut prepared = rqrr::PreparedImage::prepare(image.clone());
    prepared
        .detect_grids()
        .into_iter()
        .filter_map(|grid| grid.decode().ok().map(|(_meta, content)| content))
        .collect()
}

/// Whether an image contains at least one QR code, without decoding what it
/// holds. Used by the screen-scan UI to tell the user nothing was found.
#[must_use]
pub fn contains_qr_code(image: &DynamicImage) -> bool {
    !scan_payloads(&image.to_luma8()).is_empty()
}

#[cfg(test)]
mod tests {
    use image::{ImageFormat, Luma};

    use super::*;

    /// A synthetic secret used only to build test QR codes. Not a credential.
    const SYNTHETIC_SECRET: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    /// Renders `payload` as a QR code image with a quiet zone, at a scale the
    /// detector can resolve.
    fn qr_image(payload: &str) -> DynamicImage {
        let code = qrcode::QrCode::new(payload.as_bytes()).expect("encode");
        let modules = code.to_colors();
        let width = code.width();
        const SCALE: usize = 6;
        const QUIET: usize = 4;
        let side = (width + QUIET * 2) * SCALE;
        let side_u32 = u32::try_from(side).expect("size fits");
        let mut img = GrayImage::from_pixel(side_u32, side_u32, Luma([255u8]));
        for (index, color) in modules.iter().enumerate() {
            if *color != qrcode::Color::Dark {
                continue;
            }
            let mx = index % width;
            let my = index / width;
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let x = u32::try_from((mx + QUIET) * SCALE + dx).expect("fits");
                    let y = u32::try_from((my + QUIET) * SCALE + dy).expect("fits");
                    img.put_pixel(x, y, Luma([0u8]));
                }
            }
        }
        DynamicImage::ImageLuma8(img)
    }

    fn png_bytes(image: &DynamicImage) -> Vec<u8> {
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
            .expect("encode png");
        out
    }

    fn synthetic_uri() -> String {
        format!(
            "otpauth://totp/Example%20Co:alice@example.test?secret={SYNTHETIC_SECRET}\
             &issuer=Example%20Co&algorithm=SHA256&digits=8&period=45"
        )
    }

    #[test]
    fn a_synthetic_totp_qr_code_round_trips_through_a_png() {
        let bytes = png_bytes(&qr_image(&synthetic_uri()));
        let credential = credential_from_image_bytes(&bytes).expect("decode");
        assert_eq!(credential.parameters.issuer.as_deref(), Some("Example Co"));
        assert_eq!(
            credential.parameters.account_label.as_deref(),
            Some("alice@example.test")
        );
        assert_eq!(credential.parameters.algorithm, tw_domain::OtpAlgorithm::Sha256);
        assert_eq!(credential.parameters.digits, tw_domain::OtpDigits::Eight);
        assert_eq!(credential.parameters.period_seconds, 45);
        assert_eq!(credential.secret.len(), 20);
    }

    #[test]
    fn the_decoded_credential_generates_the_expected_code() {
        // End to end: QR image -> credential -> live TOTP, checked against the
        // same parameters generated directly.
        let bytes = png_bytes(&qr_image(&synthetic_uri()));
        let credential = credential_from_image_bytes(&bytes).expect("decode");
        let direct = tw_otp::parse_otpauth_uri(&synthetic_uri()).expect("parse");
        let from_qr = tw_otp::totp_at(&credential.secret, &credential.parameters, 1_700_000_000);
        let from_uri = tw_otp::totp_at(&direct.secret, &direct.parameters, 1_700_000_000);
        assert_eq!(from_qr, from_uri);
        assert_eq!(from_qr.code.len(), 8);
    }

    #[test]
    fn a_qr_code_can_be_read_from_a_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("factor.png");
        std::fs::write(&path, png_bytes(&qr_image(&synthetic_uri()))).expect("write");
        assert!(credential_from_image_file(&path).is_ok());
        assert!(matches!(
            credential_from_image_file(&dir.path().join("missing.png")),
            Err(QrError::FileUnreadable)
        ));
    }

    #[test]
    fn a_clipboard_style_rgba_buffer_decodes() {
        let rgba = qr_image(&synthetic_uri()).to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let credential = credential_from_rgba(w, h, rgba.as_raw()).expect("decode");
        assert_eq!(credential.parameters.issuer.as_deref(), Some("Example Co"));
    }

    #[test]
    fn a_mismatched_rgba_buffer_is_rejected_rather_than_panicking() {
        assert!(matches!(
            credential_from_rgba(10, 10, &[0u8; 4]),
            Err(QrError::MalformedPixels)
        ));
        assert!(matches!(
            credential_from_rgba(0, 10, &[]),
            Err(QrError::ImageTooLarge)
        ));
        assert!(matches!(
            credential_from_rgba(MAX_IMAGE_DIMENSION + 1, 1, &[]),
            Err(QrError::ImageTooLarge)
        ));
    }

    #[test]
    fn an_image_with_no_qr_code_says_so() {
        let blank = DynamicImage::ImageLuma8(GrayImage::from_pixel(400, 400, Luma([255u8])));
        assert!(matches!(credential_from_image(&blank), Err(QrError::NoQrCode)));
        assert!(!contains_qr_code(&blank));
        assert!(contains_qr_code(&qr_image(&synthetic_uri())));
    }

    #[test]
    fn a_non_otpauth_qr_code_is_refused_without_echoing_its_contents() {
        let payload = "https://intranet.example.test/very-private-path?token=SENSITIVE";
        let err = credential_from_image(&qr_image(payload)).expect_err("must refuse");
        assert!(matches!(err, QrError::NotAnOtpauthUri));
        let rendered = format!("{err} {err:?}");
        assert!(!rendered.contains("SENSITIVE"), "{rendered}");
        assert!(!rendered.contains("intranet"), "{rendered}");
    }

    #[test]
    fn a_malformed_otpauth_qr_code_reports_the_structural_problem_only() {
        let payload = format!("otpauth://totp/a?secret={SYNTHETIC_SECRET}&digits=7");
        let err = credential_from_image(&qr_image(&payload)).expect_err("must refuse");
        assert!(matches!(
            err,
            QrError::UnusableCredential(OtpUriError::UnsupportedDigits)
        ));
        let rendered = format!("{err} {err:?}");
        assert!(!rendered.contains(SYNTHETIC_SECRET), "{rendered}");
    }

    #[test]
    fn non_image_bytes_are_rejected() {
        assert!(matches!(
            credential_from_image_bytes(b"this is not an image"),
            Err(QrError::NotAnImage)
        ));
        assert!(matches!(
            credential_from_image_bytes(&[]),
            Err(QrError::NotAnImage)
        ));
    }

    #[test]
    fn oversized_input_is_refused_before_decoding() {
        let huge = vec![0u8; usize::try_from(MAX_IMAGE_FILE_BYTES).expect("fits") + 1];
        assert!(matches!(
            credential_from_image_bytes(&huge),
            Err(QrError::FileTooLarge)
        ));
    }

    #[test]
    fn errors_map_to_stable_diagnostic_codes() {
        assert_eq!(QrError::NoQrCode.code(), DiagnosticCode::QrNotFound);
        assert_eq!(
            QrError::NotAnOtpauthUri.code(),
            DiagnosticCode::QrPayloadNotOtpauth
        );
        assert_eq!(QrError::FileTooLarge.code(), DiagnosticCode::IoFailed);
    }
}
