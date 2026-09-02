//! QR decoding for 2FA imports (image bytes to payload text).
//!
//! The crate is deliberately generic: it returns the decoded string(s),
//! never interpreting them. The `otpauth://` parsing lives in the otp
//! crate, so no QR-specific code can ever log or reject payloads with
//! secret content. Payloads are returned exactly once per code and never
//! appear in error messages.

#![forbid(unsafe_code)]

#[allow(unused_imports)]
use image::ImageEncoder as _;
use std::io::Cursor;

use image::ImageReader;
use rqrr::{DeQRError, PreparedImage};

/// Typed QR decoding errors. No variant carries the (potentially secret)
/// payload or raw image bytes.
#[derive(Debug, thiserror::Error)]
pub enum QrError {
    /// The bytes were not a decodable image.
    #[error("not a decodable image")]
    InvalidImage,
    /// rqrr failed while decoding a detected grid.
    #[error("qr decoding failed")]
    Decode,
    /// No QR code was found in the image.
    #[error("no QR code found in the image")]
    NoCodeFound,
    /// More than one QR code decoded; the target is ambiguous.
    #[error("found multiple QR codes ({count}); crop the image to a single code")]
    MultipleCodes {
        /// Number of successfully decoded codes.
        count: usize,
    },
}

impl From<DeQRError> for QrError {
    fn from(_: DeQRError) -> Self {
        Self::Decode
    }
}

/// Decodes every QR code found in the image bytes (PNG/JPEG/etc.).
///
/// Payloads are returned in detection order; they must be treated as
/// secret material by the caller (an `otpauth://` payload contains a
/// seed).
pub fn decode_codes(image_bytes: &[u8]) -> Result<Vec<String>, QrError> {
    let image = ImageReader::new(Cursor::new(image_bytes))
        .with_guessed_format()
        .map_err(|_| QrError::InvalidImage)?
        .decode()
        .map_err(|_| QrError::InvalidImage)?;
    let luma = image.to_luma8();
    let mut prepared = PreparedImage::prepare(luma);
    let grids = prepared.detect_grids();
    let mut payloads = Vec::new();
    for grid in &grids {
        if let Ok((_, content)) = grid.decode() {
            payloads.push(content);
        }
    }
    Ok(payloads)
}

/// Decodes the single QR code the image is expected to contain.
pub fn decode_single(image_bytes: &[u8]) -> Result<String, QrError> {
    let payloads = decode_codes(image_bytes)?;
    match payloads.len() {
        0 => Err(QrError::NoCodeFound),
        1 => Ok(payloads.into_iter().next().expect("exactly one")),
        count => Err(QrError::MultipleCodes { count }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic otpauth payloads (RFC 4226 seed) are baked into the PNG
    // fixtures; nothing here is a real credential.
    const TOTP_PAYLOAD_PREFIX: &str = "otpauth://totp/TestWorkspace:alice@example.com";
    const HOTP_PAYLOAD_PREFIX: &str = "otpauth://hotp/TestWorkspace:counter@example.com";

    #[test]
    fn decodes_synthetic_totp_qr() {
        let bytes = include_bytes!("../tests/data/otpauth_totp.png");
        let payload = decode_single(bytes).expect("decodable");
        assert!(payload.starts_with(TOTP_PAYLOAD_PREFIX));
        let codes = decode_codes(bytes).expect("decodable");
        assert_eq!(codes.len(), 1);
    }

    #[test]
    fn decodes_synthetic_hotp_qr() {
        let bytes = include_bytes!("../tests/data/otpauth_hotp.png");
        let payload = decode_single(bytes).expect("decodable");
        assert!(payload.starts_with(HOTP_PAYLOAD_PREFIX));
    }

    #[test]
    fn errors_carry_no_payload() {
        let bytes = include_bytes!("../tests/data/otpauth_totp.png");
        // Corrupt the image signature so the decoder rejects the bytes.
        let mut broken = bytes.to_vec();
        broken[..12].copy_from_slice(b"NOTANIMAGESM");
        let error = decode_single(&broken).expect_err("must fail");
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains("GEZDGNBV"), "payload leaked: {rendered}");
        assert!(matches!(
            error,
            QrError::InvalidImage | QrError::NoCodeFound
        ));
    }

    #[test]
    fn garbage_bytes_are_rejected() {
        let error = decode_single(b"this is not an image at all").expect_err("must fail");
        assert!(matches!(
            error,
            QrError::InvalidImage | QrError::NoCodeFound | QrError::Decode
        ));
    }

    #[test]
    fn image_without_code_reports_no_code_found() {
        // 64x64 white PNG bytes: valid image, no QR code.
        let white = image::GrayImage::from_pixel(64, 64, image::Luma([255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(Cursor::new(&mut png))
            .write_image(&white, 64, 64, image::ExtendedColorType::L8)
            .expect("encode");
        let error = decode_single(&png).expect_err("no code present");
        assert!(matches!(error, QrError::NoCodeFound), "got: {error:?}");
    }
}
