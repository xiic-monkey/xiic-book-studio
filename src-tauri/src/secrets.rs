use std::process::Command;

use crate::error::{AppError, AppResult};

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
fn set_api_key_with_user(user: &str, api_key: &str) -> AppResult<()> {
    // `-U` updates an existing item or creates it. Deleting first can lose the
    // working credential when Keychain locks between the two commands.
    let output = Command::new("security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            KEYRING_SERVICE,
            "-a",
            user,
            "-w",
            api_key,
        ])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "保存 API Key 到 Keychain 失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )))
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

#[cfg(not(target_os = "macos"))]
fn set_api_key_with_user(_user: &str, _api_key: &str) -> AppResult<()> {
    Err(AppError::Validation(
        "当前版本的系统凭据存储先支持 macOS Keychain".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
fn get_api_key_with_user(_user: &str) -> AppResult<Option<String>> {
    Ok(None)
}

pub fn set_api_key(api_key: &str) -> AppResult<()> {
    set_api_key_with_user(LEGACY_KEYRING_USER, api_key)
}

pub fn set_api_key_for_scope(scope: &str, api_key: &str) -> AppResult<()> {
    let user = keyring_user_for_scope(scope);
    set_api_key_with_user(&user, api_key)
}

pub fn get_api_key() -> AppResult<Option<String>> {
    get_api_key_with_user(LEGACY_KEYRING_USER)
}

pub fn get_api_key_for_scope(scope: &str) -> AppResult<Option<String>> {
    let user = keyring_user_for_scope(scope);
    get_api_key_with_user(&user)
}
