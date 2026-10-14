//! Windows: the sign-in in a file that only this user of this machine can decrypt (DPAPI, the protection
//! behind the Credential Manager). A file rather than a Credential Manager entry because an entry holds
//! 2560 bytes at most, and a refresh token is free to be longer.

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

use crate::platform::{PlatformError, Result, TokenStore};

pub struct ProtectedFile {
    path: PathBuf,
}

impl ProtectedFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl TokenStore for ProtectedFile {
    fn load(&self) -> Result<Option<String>> {
        let protected = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error("reading the stored sign-in", error)),
        };
        // Written by another user, on another machine, or cut short: as good as not there.
        Ok(unprotect(&protected).and_then(|bytes| String::from_utf8(bytes).ok()))
    }

    fn save(&self, refresh_token: &str) -> Result<()> {
        let protected = protect(refresh_token.as_bytes())?;
        if let Some(directory) = self.path.parent() {
            fs::create_dir_all(directory)
                .map_err(|error| io_error("creating the directory of the stored sign-in", error))?;
        }

        // Written beside the file and moved over it, so that the file is never half of a token.
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(".new");
        fs::write(&temporary, protected)
            .and_then(|()| fs::rename(&temporary, &self.path))
            .map_err(|error| io_error("writing the stored sign-in", error))
    }

    fn clear(&self) -> Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error("removing the stored sign-in", error)),
        }
    }
}

fn io_error(operation: &'static str, error: std::io::Error) -> PlatformError {
    PlatformError::Os {
        operation,
        detail: error.to_string(),
    }
}

fn protect(plain: &[u8]) -> Result<Vec<u8>> {
    let input = blob(plain);
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| PlatformError::Os {
        operation: "CryptProtectData",
        detail: error.message(),
    })?;
    Ok(take(output))
}

fn unprotect(protected: &[u8]) -> Option<Vec<u8>> {
    let input = blob(protected);
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .ok()?;
    Some(take(output))
}

fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    }
}

/// Copies what the system allocated for a result and frees it.
fn take(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> ProtectedFile {
        let directory =
            std::env::temp_dir().join(format!("perora-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        // One level that does not exist yet: saving creates it.
        ProtectedFile::new(directory.join("nested").join("sign-in"))
    }

    #[test]
    fn what_is_saved_comes_back_and_is_not_on_disk_as_it_was() {
        let store = store("round-trip");
        assert_eq!(store.load().unwrap(), None);

        // Longer than a Credential Manager entry could hold.
        let token = "refresh-token-".repeat(300);
        store.save(&token).unwrap();

        assert_eq!(store.load().unwrap().as_deref(), Some(token.as_str()));
        let on_disk = fs::read(&store.path).unwrap();
        assert!(
            !on_disk
                .windows(14)
                .any(|window| window == b"refresh-token-")
        );

        store.save("the next one").unwrap();
        assert_eq!(store.load().unwrap().as_deref(), Some("the next one"));

        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
        store.clear().unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_decrypted_is_no_sign_in() {
        let store = store("garbage");
        store.save("token").unwrap();
        fs::write(&store.path, b"not what was written").unwrap();

        assert_eq!(store.load().unwrap(), None);
    }
}
