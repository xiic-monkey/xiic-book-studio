//! API Key 字段级对称加密（方案 B）。
//!
//! 设计：每个安装实例在数据库同级目录生成一个随机 `device.key`（32 字节，
//! 作为 AES-256-GCM 的密钥），密钥不写入 SQLite 库内。每条 `ai_providers.api_key`
//! 落库时以 `ENC1::<base64(nonce||ciphertext)>` 形式存储，读取时解密。
//! 前缀 `ENC1::` 用于区分密文与遗留明文，避免歧义。

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

const NONCE_LEN: usize = 12;
const DEVICE_KEY_LEN: usize = 32;
const DEVICE_KEY_FILE: &str = "device.key";

/// 密文存储前缀；`ENC1::` 后接 base64(nonce || ciphertext)。
pub const ENCRYPTED_PREFIX: &str = "ENC1::";

/// 判断存储值是否已是本方案密文。
pub fn is_encrypted(stored: &str) -> bool {
    stored.starts_with(ENCRYPTED_PREFIX)
}

pub struct ApiKeyCipher {
    key: [u8; DEVICE_KEY_LEN],
}

impl Clone for ApiKeyCipher {
    fn clone(&self) -> Self {
        Self { key: self.key }
    }
}

impl ApiKeyCipher {
    /// 从数据库路径派生设备密钥文件路径并加载/创建设备密钥。
    pub fn new(db_path: &Path) -> AppResult<Self> {
        let device_key_path = device_key_path(db_path);
        let device_key = load_or_create_device_key(&device_key_path)?;
        Ok(Self { key: device_key })
    }

    /// 返回 `ENC1::<base64(nonce||ciphertext)>`。
    pub fn encrypt(&self, plaintext: &str) -> AppResult<String> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|e| AppError::Validation(format!("加密 API Key 失败: {e}")))?;
        let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);
        Ok(format!("{ENCRYPTED_PREFIX}{}", BASE64.encode(combined)))
    }

    /// 解密 `ENC1::<base64(nonce||ciphertext)>` 得到明文。
    pub fn decrypt(&self, stored: &str) -> AppResult<String> {
        let payload = stored
            .strip_prefix(ENCRYPTED_PREFIX)
            .ok_or_else(|| AppError::Validation("API Key 不是预期密文格式".to_string()))?;
        let combined = BASE64
            .decode(payload)
            .map_err(|e| AppError::Validation(format!("API Key base64 解析失败: {e}")))?;
        if combined.len() <= NONCE_LEN {
            return Err(AppError::Validation("API Key 密文长度无效".to_string()));
        }
        let (nonce, ciphertext) = combined.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|e| AppError::Validation(format!("解密 API Key 失败: {e}")))?;
        String::from_utf8(plaintext)
            .map_err(|e| AppError::Validation(format!("API Key 解密结果不是有效 UTF-8: {e}")))
    }
}

fn device_key_path(db_path: &Path) -> PathBuf {
    let dir = db_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(DEVICE_KEY_FILE)
}

fn load_or_create_device_key(path: &Path) -> AppResult<[u8; DEVICE_KEY_LEN]> {
    if let Ok(bytes) = fs::read(path) {
        if bytes.len() == DEVICE_KEY_LEN {
            let mut key = [0u8; DEVICE_KEY_LEN];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
        eprintln!(
            "设备密钥文件大小异常（{} 字节），将重新生成: {}",
            bytes.len(),
            path.display()
        );
    }
    let mut key = [0u8; DEVICE_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(path, key).map_err(|e| {
        AppError::Validation(format!("无法写入设备密钥文件 {}: {e}", path.display()))
    })?;
    Ok(key)
}
