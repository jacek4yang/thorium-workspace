//! Key derivation and AEAD primitives for the vault.
//!
//! Only audited RustCrypto primitives are used:
//! - Argon2id (RFC 9106) for memory-hard master-key derivation;
//! - ChaCha20-Poly1305 (RFC 8439) for authenticated encryption.
//!
//! No custom cryptographic constructions appear here.

use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce as AeadNonce,
    aead::{Aead, Payload},
};
use zeroize::{Zeroize, Zeroizing};

use crate::error::VaultError;
use crate::format::{HEADER_LEN, KdfParams, NONCE_LEN, SALT_LEN};

/// Derived vault master key. Zeroized on drop.
pub(crate) struct VaultKey(Zeroizing<[u8; 32]>);

impl VaultKey {
    /// Derives the master key from a master password and salt using the
    /// given Argon2id parameters.
    pub fn derive(
        master_password: &thorium_workspace_secrets::SecretText,
        salt: &[u8],
        kdf: &KdfParams,
    ) -> Result<Self, VaultError> {
        let params = argon2::Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.parallelism, Some(32))
            .map_err(|error| VaultError::Kdf(error.to_string()))?;
        let argon =
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut okm = [0u8; 32];
        argon
            .hash_password_into(master_password.expose().as_bytes(), salt, &mut okm)
            .map_err(|error| VaultError::Kdf(error.to_string()))?;
        Ok(Self(Zeroizing::new(okm)))
    }
}

/// Fills `buffer` with cryptographically secure random bytes from the
/// operating system source.
pub(crate) fn fill_random(buffer: &mut [u8]) -> Result<(), VaultError> {
    use rand::TryRng;
    rand::rngs::SysRng
        .try_fill_bytes(buffer)
        .map_err(|error| VaultError::RandomSource(error.to_string()))
}

/// Fresh random salt for a new vault or password change.
pub(crate) fn fresh_salt() -> Result<[u8; SALT_LEN], VaultError> {
    let mut salt = [0u8; SALT_LEN];
    fill_random(&mut salt)?;
    Ok(salt)
}

/// Fresh random nonce for one encryption operation.
pub(crate) fn fresh_nonce() -> Result<[u8; NONCE_LEN], VaultError> {
    let mut nonce = [0u8; NONCE_LEN];
    fill_random(&mut nonce)?;
    Ok(nonce)
}

/// Encrypts `plaintext` with the key, authenticating `header_bytes` as
/// additional data. The header must be exactly [`HEADER_LEN`] bytes (the
/// same bytes stored in the file).
pub(crate) fn encrypt(
    key: &VaultKey,
    nonce_bytes: &[u8; NONCE_LEN],
    header_bytes: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    debug_assert_eq!(header_bytes.len(), HEADER_LEN);
    let cipher = ChaCha20Poly1305::new(&Key::from(*key.0));
    let nonce = AeadNonce::from(*nonce_bytes);
    cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: header_bytes,
            },
        )
        .map_err(|_| VaultError::UnlockFailed)
}

/// Decrypts `ciphertext` with the key, authenticating `header_bytes`.
///
/// Returns the plaintext JSON bytes; the caller must zeroize them after
/// deserialization.
pub(crate) fn decrypt(
    key: &VaultKey,
    nonce_bytes: &[u8; NONCE_LEN],
    header_bytes: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key.0));
    let nonce = AeadNonce::from(*nonce_bytes);
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad: header_bytes,
            },
        )
        .map_err(|_| VaultError::UnlockFailed)
}

/// Zeroizes a decrypted plaintext buffer after use.
pub(crate) fn scrub(buffer: &mut [u8]) {
    buffer.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;
    use thorium_workspace_secrets::SecretText;

    const SYNTHETIC_PASSWORD: &str = "synthetic-master-password-1";
    const SYNTHETIC_PAYLOAD: &[u8] = b"{\"synthetic\":true}";

    #[test]
    fn encrypt_decrypt_roundtrips_with_aad() {
        let salt = fresh_salt().expect("salt");
        let kdf = KdfParams {
            m_cost_kib: 8 * 1024,
            t_cost: 1,
            parallelism: 1,
        };
        let key = VaultKey::derive(&SecretText::new(SYNTHETIC_PASSWORD), &salt, &kdf)
            .expect("derivation");
        let nonce = fresh_nonce().expect("nonce");
        let header = {
            use crate::format::VaultHeader;
            VaultHeader { salt, kdf, nonce }.encoded()
        };
        let ciphertext = encrypt(&key, &nonce, &header, SYNTHETIC_PAYLOAD).expect("encrypt");
        assert!(
            !ciphertext
                .windows(SYNTHETIC_PAYLOAD.len())
                .any(|w| w == SYNTHETIC_PAYLOAD)
        );

        let plaintext = decrypt(&key, &nonce, &header, &ciphertext).expect("decrypt");
        assert_eq!(plaintext, SYNTHETIC_PAYLOAD);
        let mut plaintext = plaintext;
        scrub(&mut plaintext);
        assert!(plaintext.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn tampered_aad_or_ciphertext_fails_authentication() {
        let salt = fresh_salt().expect("salt");
        let kdf = KdfParams {
            m_cost_kib: 8 * 1024,
            t_cost: 1,
            parallelism: 1,
        };
        let key = VaultKey::derive(&SecretText::new(SYNTHETIC_PASSWORD), &salt, &kdf)
            .expect("derivation");
        let nonce = fresh_nonce().expect("nonce");
        let mut header = {
            use crate::format::VaultHeader;
            VaultHeader { salt, kdf, nonce }
        }
        .encoded();
        let ciphertext = encrypt(&key, &nonce, &header, SYNTHETIC_PAYLOAD).expect("encrypt");

        // Flip one header byte (simulated KDF downgrade/tamper).
        let mut tampered_header = header.clone();
        tampered_header[44] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &nonce, &tampered_header, &ciphertext),
            Err(VaultError::UnlockFailed)
        ));
        header[44] ^= 0x01; // restore

        // Flip one ciphertext byte.
        let mut tampered = ciphertext.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &nonce, &header, &tampered),
            Err(VaultError::UnlockFailed)
        ));
    }

    #[test]
    fn different_passwords_derive_different_keys() {
        let salt = fresh_salt().expect("salt");
        let kdf = KdfParams {
            m_cost_kib: 8 * 1024,
            t_cost: 1,
            parallelism: 1,
        };
        let a = VaultKey::derive(&SecretText::new("synthetic-A"), &salt, &kdf).expect("a");
        let b = VaultKey::derive(&SecretText::new("synthetic-B"), &salt, &kdf).expect("b");
        let nonce = fresh_nonce().expect("nonce");
        let header = {
            use crate::format::VaultHeader;
            VaultHeader { salt, kdf, nonce }.encoded()
        };
        let ciphertext = encrypt(&a, &nonce, &header, SYNTHETIC_PAYLOAD).expect("encrypt");
        assert!(matches!(
            decrypt(&b, &nonce, &header, &ciphertext),
            Err(VaultError::UnlockFailed)
        ));
    }

    #[test]
    fn random_material_is_not_repeated() {
        let a = fresh_salt().expect("salt");
        let b = fresh_salt().expect("salt");
        assert_ne!(a, b);
        let n1 = fresh_nonce().expect("nonce");
        let n2 = fresh_nonce().expect("nonce");
        assert_ne!(n1, n2);
    }
}
