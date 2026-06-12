//! Credential storage for connection secrets.
//!
//! On macOS, secrets are stored in the **data-protection keychain** guarded by a
//! Touch ID / passcode access control (`USER_PRESENCE`), so reading a secret
//! prompts for fingerprint (with automatic password fallback). With our
//! in-memory cache that means at most one Touch ID prompt per connection per app
//! launch.
//!
//! That biometric path REQUIRES a code-signed app carrying a
//! `keychain-access-groups` entitlement. On an unsigned / dev build the calls
//! fail immediately with `errSecMissingEntitlement` (before any prompt), so we
//! transparently fall back to the legacy keychain (the `keyring` crate). The app
//! therefore keeps working without signing — just without the Touch ID prompt.
//!
//! NOTE: the biometric branch is unverified in CI/dev (it can only run in a
//! signed build). See the signing steps in the project docs to activate it.
//!
//! In debug builds the app uses a plaintext dev file instead (see
//! `connections.rs`), so this module is unused there — hence the allow below.
#![allow(dead_code)]

use crate::error::{Error, Result};

const SERVICE: &str = "io.cirrus.novacirrus";

/// Store `json` (the serialized secret) for connection `id`.
pub fn write(id: &str, json: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if mac::write(id, json.as_bytes()) {
            return Ok(());
        }
        // Biometric keychain unavailable (e.g. unsigned build) → legacy keychain.
    }
    legacy::write(id, json)
}

/// Read the stored secret JSON for `id`, or `None` if there is none.
pub fn read(id: &str) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        match mac::read(id) {
            mac::Read::Found(bytes) => {
                return Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
            }
            // User actively cancelled/failed Touch ID — do NOT silently fall back
            // to an unprotected copy.
            mac::Read::Denied => {
                return Err(Error::Msg(
                    "Biometric authentication was cancelled — secret not unlocked".into(),
                ))
            }
            // No biometric item, or the biometric keychain isn't usable here →
            // try the legacy keychain.
            mac::Read::NotFound | mac::Read::Unsupported => {}
        }
    }
    legacy::read(id)
}

/// Remove any stored secret for `id` (best-effort across both backends).
pub fn clear(id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        mac::clear(id);
    }
    legacy::clear(id)
}

/// Legacy, non-biometric keychain via the cross-platform `keyring` crate. Used
/// on non-macOS targets and as the macOS fallback for unsigned builds.
mod legacy {
    use super::{Error, Result, SERVICE};

    fn entry(id: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(SERVICE, id)?)
    }

    pub fn write(id: &str, json: &str) -> Result<()> {
        entry(id)?.set_password(json)?;
        Ok(())
    }

    pub fn read(id: &str) -> Result<Option<String>> {
        match entry(id)?.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Keyring(e)),
        }
    }

    pub fn clear(id: &str) -> Result<()> {
        match entry(id)?.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Keyring(e)),
        }
    }
}

/// macOS biometric (Touch ID) backend over the data-protection keychain.
#[cfg(target_os = "macos")]
mod mac {
    use super::SERVICE;
    use security_framework::passwords::{
        delete_generic_password_options, generic_password, set_generic_password_options,
        AccessControlOptions, PasswordOptions,
    };

    // OSStatus codes we special-case.
    const ERR_ITEM_NOT_FOUND: i32 = -25300;
    const ERR_MISSING_ENTITLEMENT: i32 = -34018; // unsigned / no keychain-access-groups
    const ERR_PARAM: i32 = -50;
    const ERR_NOT_AVAILABLE: i32 = -25291;
    const ERR_USER_CANCELED: i32 = -128;
    const ERR_AUTH_FAILED: i32 = -25293;

    pub enum Read {
        Found(Vec<u8>),
        /// No biometric item exists for this id.
        NotFound,
        /// The data-protection / biometric keychain isn't usable here (e.g.
        /// unsigned build, missing entitlement) — caller should try legacy.
        Unsupported,
        /// The user cancelled or failed Touch ID — surface, do not fall back.
        Denied,
    }

    /// Base query targeting the data-protection keychain (required for access
    /// control). Carries no access-control flag, which is correct for read/delete.
    fn base_opts(id: &str) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(SERVICE, id);
        o.use_protected_keychain();
        o
    }

    fn is_unsupported(code: i32) -> bool {
        matches!(code, ERR_MISSING_ENTITLEMENT | ERR_PARAM | ERR_NOT_AVAILABLE)
    }

    /// Returns true if the secret was stored in the biometric keychain.
    pub fn write(id: &str, secret: &[u8]) -> bool {
        // SecItemAdd fails if the item already exists; replace it.
        let _ = delete_generic_password_options(base_opts(id));
        let mut o = base_opts(id);
        // Touch ID or device passcode required to read this item.
        o.set_access_control_options(AccessControlOptions::USER_PRESENCE);
        set_generic_password_options(secret, o).is_ok()
    }

    pub fn read(id: &str) -> Read {
        match generic_password(base_opts(id)) {
            Ok(bytes) => Read::Found(bytes),
            Err(e) => match e.code() {
                ERR_ITEM_NOT_FOUND => Read::NotFound,
                ERR_USER_CANCELED | ERR_AUTH_FAILED => Read::Denied,
                // Missing entitlement / not available / anything unexpected:
                // prefer availability and fall back to the legacy keychain.
                code if is_unsupported(code) => Read::Unsupported,
                _ => Read::Unsupported,
            },
        }
    }

    pub fn clear(id: &str) {
        let _ = delete_generic_password_options(base_opts(id));
    }
}
