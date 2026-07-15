use pwhash::bcrypt;

use crate::errors::{XenovraStreamError, XenovraStreamResult};

pub struct PasswordManager;

impl PasswordManager {
    pub fn generate(password: &str) -> XenovraStreamResult<String> {
        bcrypt::hash(password).map_err(|e| {
            tracing::error!("{e}");
            XenovraStreamError::Unknown
        })
    }

    pub fn verify(password: &str, hash: &str) -> XenovraStreamResult<()> {
        if bcrypt::verify(password, hash) {
            Ok(())
        } else {
            Err(XenovraStreamError::NotAuthenticated)
        }
    }
}
