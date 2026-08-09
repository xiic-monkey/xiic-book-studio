use std::process::Command;

use crate::error::{AppError, AppResult};

// This module is only used once to migrate legacy Keychain entries into SQLite.

const KEYRING_SERVICE: &str = "xiic-book-studio";
const LEGACY_KEYRING_USER: &str = "openai-compatible-api-key";

fn keyring_user_for_scope(scope: &str) -> String {
    let normalized = scope.trim().trim_end_matches('/').to_lowercase();
    if normalized.is_empty() {
        LEGACY_KEYRING_USER.to_string()
    } else {
        format!("{LEGACY_KEYRING_USER}::{normalized}")
    }
}

#[cfg(target_os = "macos")]
fn get_api_key_with_user(user: &str) -> AppResult<Option<String>> {
    let output = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYRING_SERVICE,
            "-a",
            user,
            "-w",
        ])
        .output()?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!value.is_empty()).then_some(value))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn clear_api_key_with_user(user: &str) -> AppResult<()> {
    let output = Command::new("security")
        .args(["delete-generic-password", "-s", KEYRING_SERVICE, "-a", user])
        .output()?;

    if output.status.success()
        || String::from_utf8_lossy(&output.stderr).contains("could not be found")
    {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "删除 Keychain API Key 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

#[cfg(not(target_os = "macos"))]
fn get_api_key_with_user(_user: &str) -> AppResult<Option<String>> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn clear_api_key_with_user(_user: &str) -> AppResult<()> {
    Ok(())
}

pub(crate) fn get_api_key() -> AppResult<Option<String>> {
    get_api_key_with_user(LEGACY_KEYRING_USER)
}

pub(crate) fn get_api_key_for_scope(scope: &str) -> AppResult<Option<String>> {
    let user = keyring_user_for_scope(scope);
    get_api_key_with_user(&user)
}

pub(crate) fn clear_api_key_for_scope(scope: &str) -> AppResult<()> {
    let user = keyring_user_for_scope(scope);
    clear_api_key_with_user(&user)
}

pub(crate) fn clear_api_key() -> AppResult<()> {
    clear_api_key_with_user(LEGACY_KEYRING_USER)
}
