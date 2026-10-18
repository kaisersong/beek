# Windows implementation review snapshot

Only this subsystem is in scope. Full G5 is NOT passed. G0-D allows internal development. Do not treat missing external signing/multi-monitor/soak evidence as a code bug, and do not approve public release. Review adversarially for concrete reproducible bugs, not generic concerns. Read permission includes continuing this same attachment if truncated.

## src/core/settings.rs
```
//! Windows settings contract. Credentials are immutable references; the full
//! provider snapshot and its reference commit together, then the old secret retires.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Provider {
    pub credential_ref: Option<String>,
    pub model: String,
    pub base_url: String,
    // Read legacy plaintext once for migration; never emit it in new files.
    #[serde(skip_serializing)]
    pub api_key: String,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            credential_ref: None,
            model: super::prompts::DEFAULT_MODEL.into(),
            base_url: super::prompts::DEFAULT_ENDPOINT.into(),
            api_key: String::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    pub selection_enabled: bool,
    pub copy_to_popup: bool,
    pub autostart: bool,
    pub hotkey_enabled: bool,
    pub hotkey: String,
    pub provider: Provider,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            selection_enabled: true,
            copy_to_popup: false,
            autostart: false,
            hotkey_enabled: true,
            hotkey: "Ctrl+Alt+T".into(),
            provider: Provider::default(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("不支持的配置版本，请使用相应版本 BeeK".into());
        }
        if self.provider.model.trim().is_empty() || self.provider.model.len() > 1024 {
            return Err("模型名称无效".into());
        }
        let url = reqwest::Url::parse(&self.provider.base_url).map_err(|_| "API 地址无效")?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err("API 地址需为 HTTPS（本机测试可用 HTTP）".into());
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err("API 地址不能包含账号、密码或片段".into());
        }
        Ok(())
    }
}
pub enum KeyEdit {
    Keep,
    Replace(String),
    Clear,
}
pub trait Backend {
    fn create_secret(&mut self, value: &str) -> Result<String, String>;
    fn read_secret(&self, reference: &str) -> Result<String, String>;
    fn delete_secret(&mut self, reference: &str) -> Result<(), String>;
    fn commit(&mut self, settings: &Settings) -> Result<(), String>;
}
pub fn transact(
    store: &mut impl Backend,
    old: &Settings,
    mut next: Settings,
    edit: KeyEdit,
) -> Result<Settings, String> {
    next.validate()?;
    let staged = match edit {
        KeyEdit::Keep => {
            next.provider.credential_ref = old.provider.credential_ref.clone();
            None
        }
        KeyEdit::Clear => {
            next.provider.credential_ref = None;
            None
        }
        KeyEdit::Replace(value) => {
            let value = value.trim();
            if value.is_empty() || value.len() > 2560 {
                return Err("API Key 为空或过长".into());
            }
            let reference = store.create_secret(value)?;
            match store.read_secret(&reference) {
                Ok(actual) if actual == value => {}
                _ => {
                    let _ = store.delete_secret(&reference);
                    return Err("凭据写入后复核失败，原配置已保留".into());
                }
            }
            next.provider.credential_ref = Some(reference.clone());
            Some(reference)
        }
    };
    next.provider.api_key.clear();
    if let Err(e) = store.commit(&next) {
        if let Some(r) = staged {
            let _ = store.delete_secret(&r);
        }
        return Err(e);
    }
    if old.provider.credential_ref != next.provider.credential_ref {
        if let Some(r) = &old.provider.credential_ref {
            let _ = store.delete_secret(r);
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Memory {
        secrets: HashMap<String, String>,
        disk: Option<Settings>,
        fail: bool,
    }
    impl Backend for Memory {
        fn create_secret(&mut self, v: &str) -> Result<String, String> {
            let r = format!("new-{}", self.secrets.len());
            self.secrets.insert(r.clone(), v.into());
            Ok(r)
        }
        fn read_secret(&self, r: &str) -> Result<String, String> {
            self.secrets.get(r).cloned().ok_or("missing".into())
        }
        fn delete_secret(&mut self, r: &str) -> Result<(), String> {
            self.secrets.remove(r);
            Ok(())
        }
        fn commit(&mut self, s: &Settings) -> Result<(), String> {
            if self.fail {
                Err("disk full".into())
            } else {
                self.disk = Some(s.clone());
                Ok(())
            }
        }
    }
    #[test]
    fn failed_config_commit_preserves_old_key_and_provider() {
        let mut m = Memory::default();
        let old = transact(
            &mut m,
            &Settings::default(),
            Settings::default(),
            KeyEdit::Replace("old-key".into()),
        )
        .unwrap();
        let oldref = old.provider.credential_ref.clone().unwrap();
        m.fail = true;
        let mut next = old.clone();
        next.provider.model = "new-model".into();
        assert!(transact(&mut m, &old, next, KeyEdit::Replace("new-key".into())).is_err());
        assert_eq!(m.read_secret(&oldref).unwrap(), "old-key");
        assert_eq!(m.disk.unwrap().provider.model, old.provider.model);
        assert_eq!(m.secrets.len(), 1);
    }
    #[test]
    fn successful_commit_never_serializes_key_and_clear_is_explicit() {
        let mut m = Memory::default();
        let s = transact(
            &mut m,
            &Settings::default(),
            Settings::default(),
            KeyEdit::Replace("private-key".into()),
        )
        .unwrap();
        assert!(!serde_json::to_string(&s).unwrap().contains("private-key"));
        let r = s.provider.credential_ref.clone().unwrap();
        let kept = transact(&mut m, &s, s.clone(), KeyEdit::Keep).unwrap();
        assert_eq!(kept.provider.credential_ref, s.provider.credential_ref);
        let clear = transact(&mut m, &kept, kept.clone(), KeyEdit::Clear).unwrap();
        assert!(clear.provider.credential_ref.is_none());
        assert!(m.read_secret(&r).is_err());
    }
    #[test]
    fn missing_fields_do_not_enable_clipboard_or_autostart() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.copy_to_popup && !s.autostart);
        s.validate().unwrap();
    }
}

```

## src/platform/windows/store.rs
```
use super::util;
use crate::core::settings::{self, Backend, KeyEdit, Settings};
use sha2::{Digest, Sha256};
use std::os::windows::fs::OpenOptionsExt;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    sync::Mutex,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{Security::Credentials::*, Storage::FileSystem::*},
};

pub struct Store {
    path: PathBuf,
    namespace: String,
    lock: Mutex<()>,
    expected: Mutex<Option<Option<[u8; 32]>>>,
}
impl Store {
    pub fn open() -> Result<Self, String> {
        let dir = util::known_folder(false)?;
        fs::create_dir_all(&dir).map_err(|_| "无法创建配置目录")?;
        Ok(Self {
            path: dir.join("config.json"),
            namespace: "BeeK/ark/".into(),
            lock: Mutex::new(()),
            expected: Mutex::new(None),
        })
    }
    pub fn load(&self) -> Result<Settings, String> {
        let _guard = self.lock.lock().map_err(|_| "配置锁不可用")?;
        let _file_lock = self.file_lock()?;
        let metadata = match fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                *self.expected.lock().unwrap() = Some(None);
                return Ok(Settings::default());
            }
            Err(_) => return Err("无法读取配置文件".into()),
        };
        if metadata.len() > 1024 * 1024 {
            return Err("配置文件超出大小限制，原文件已保留".into());
        }
        let data = self.bytes()?.ok_or("配置文件在读取时发生变化")?;
        *self.expected.lock().unwrap() = Some(Some(Sha256::digest(&data).into()));
        let parsed = parse(&data);
        let cfg: Settings = parsed.map_err(|_| self.preserve_invalid("配置文件损坏"))?;
        cfg.validate()
            .map_err(|error| self.preserve_invalid(&error))?;
        Ok(cfg)
    }
    fn preserve_invalid(&self, reason: &str) -> String {
        let backup =
            util::nonce().map(|nonce| self.path.with_extension(format!("corrupt-{nonce}.json")));
        if backup
            .and_then(|path| fs::copy(&self.path, path).map_err(|_| "备份失败".into()))
            .is_err()
        {
            *self.expected.lock().unwrap() = None;
            format!("{reason}；无法备份原文件，请手动恢复后重新启动")
        } else {
            format!("{reason}；原文件已备份，保存恢复配置前请确认设置")
        }
    }
    pub fn save(
        &mut self,
        old: &Settings,
        next: Settings,
        key: KeyEdit,
    ) -> Result<Settings, String> {
        let _file_lock = self.file_lock()?;
        settings::transact(self, old, next, key)
    }
    fn bytes(&self) -> Result<Option<Vec<u8>>, String> {
        let file = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("无法读取配置文件".into()),
        };
        let mut data = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|_| "读取配置失败")?;
        if data.len() > 1024 * 1024 {
            return Err("配置超出大小限制，原文件已保留".into());
        }
        Ok(Some(data))
    }
    fn hash(&self) -> Result<Option<[u8; 32]>, String> {
        Ok(self.bytes()?.map(|v| Sha256::digest(v).into()))
    }
    fn file_lock(&self) -> Result<fs::File, String> {
        let path = self.path.with_extension("lock");
        let at = std::time::Instant::now();
        loop {
            if let Ok(file) = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&path)
            {
                return Ok(file);
            }
            if at.elapsed() >= std::time::Duration::from_millis(300) {
                return Err("配置正被另一会话使用或目录不可写，请稍后重试".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    fn owns(&self, reference: &str) -> bool {
        reference.strip_prefix(&self.namespace).is_some_and(|v| {
            v.len() == 48
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }
    pub fn purge_user_data() -> Result<(), String> {
        let mut store = Self::open()?;
        {
            let _guard = store.file_lock()?;
            if store.path.exists() {
                fs::remove_file(&store.path).map_err(|_| "无法删除 BeeK 配置")?;
            }
            *store.expected.lock().unwrap() = Some(None);
        }
        store.cleanup_older_than(std::time::Duration::ZERO)?;
        let roaming = util::known_folder(false)?;
        let local = util::known_folder(true)?;
        drop(store);
        for directory in [roaming, local] {
            if directory.exists() {
                fs::remove_dir_all(directory).map_err(|_| "部分 BeeK 数据无法清理")?;
            }
        }
        Ok(())
    }
    pub fn cleanup(&mut self) -> Result<(), String> {
        self.cleanup_older_than(std::time::Duration::from_secs(24 * 3600))
    }
    fn cleanup_older_than(&mut self, age: std::time::Duration) -> Result<(), String> {
        let _guard = self.file_lock()?;
        if *self.expected.lock().unwrap() != Some(self.hash()?) {
            return Err("配置已变化，延后清理".into());
        }
        let current = self
            .bytes()?
            .map(|data| parse(&data))
            .transpose()
            .map_err(|_| "配置不可读，延后清理")?
            .and_then(|cfg| cfg.provider.credential_ref);
        let filter = util::wide(format!("{}*", self.namespace));
        let mut count = 0;
        let mut credentials = std::ptr::null_mut();
        let mut failed = false;
        unsafe {
            let enumeration =
                CredEnumerateW(PCWSTR(filter.as_ptr()), None, &mut count, &mut credentials);
            if let Err(error) = &enumeration {
                if error.code()
                    != windows::core::HRESULT::from_win32(
                        windows::Win32::Foundation::ERROR_NOT_FOUND.0,
                    )
                {
                    return Err("无法枚举 BeeK 旧凭据，可稍后重试".into());
                }
            }
            if enumeration.is_ok() {
                struct List(*mut *mut CREDENTIALW);
                impl Drop for List {
                    fn drop(&mut self) {
                        unsafe {
                            CredFree(self.0 as *const _);
                        }
                    }
                }
                let _list = List(credentials);
                if count > 10000 {
                    return Err("凭据数量超出安全清理上限".into());
                }
                if count <= 10000 && !credentials.is_null() {
                    for credential in std::slice::from_raw_parts(credentials, count as usize) {
                        let c = &**credential;
                        if let Ok(reference) = c.TargetName.to_string() {
                            let written = ((c.LastWritten.dwHighDateTime as u64) << 32)
                                | c.LastWritten.dwLowDateTime as u64;
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                                .saturating_add(11644473600)
                                .saturating_mul(10_000_000);
                            if c.Type == CRED_TYPE_GENERIC
                                && self.owns(&reference)
                                && Some(&reference) != current.as_ref()
                                && now.saturating_sub(written) / 10_000_000 >= age.as_secs()
                            {
                                failed |= self.delete_secret(&reference).is_err();
                            }
                        }
                    }
                }
            }
        }
        if failed {
            return Err("部分旧凭据清理失败，可稍后重试".into());
        }
        if let Some(dir) = self.path.parent() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name
                        .strip_prefix("config.")
                        .and_then(|v| v.strip_suffix(".tmp"))
                        .is_some_and(|v| v.len() == 48 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                        && entry
                            .metadata()
                            .ok()
                            .filter(|m| m.is_file())
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|elapsed| elapsed >= age)
                    {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn key(&self, cfg: &Settings) -> Result<String, String> {
        // An explicit reference must resolve. Never hide credential loss by silently
        // falling back to another account in the process environment.
        if let Some(reference) = &cfg.provider.credential_ref {
            return self.read_secret(reference);
        }
        std::env::var("BEEK_ARK_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .ok_or("请在设置中填写 API Key".into())
    }
}
impl Backend for Store {
    fn create_secret(&mut self, value: &str) -> Result<String, String> {
        let reference = format!("{}{}", self.namespace, util::nonce()?);
        let target = util::wide(&reference);
        let user = util::wide("BeeK");
        let bytes = value.as_bytes();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_ptr() as *mut _),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_ptr() as *mut _,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_ptr() as *mut _),
            ..Default::default()
        };
        unsafe { CredWriteW(&credential, 0) }.map_err(|_| "Windows 凭据管理器写入失败")?;
        Ok(reference)
    }
    fn read_secret(&self, reference: &str) -> Result<String, String> {
        if !self.owns(reference) {
            return Err("凭据引用无效".into());
        }
        let target = util::wide(reference);
        let mut ptr = std::ptr::null_mut();
        unsafe {
            CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut ptr)
                .map_err(|_| "无法读取已保存的 API Key，请在设置中重新填写")?;
            struct Credential(*mut CREDENTIALW);
            impl Drop for Credential {
                fn drop(&mut self) {
                    unsafe {
                        CredFree(self.0 as *const _);
                    }
                }
            }
            let _guard = Credential(ptr);
            let c = &*ptr;
            if c.CredentialBlobSize > 8192 || c.CredentialBlob.is_null() {
                return Err("凭据内容无效".into());
            }
            String::from_utf8(
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                    .to_vec(),
            )
            .map_err(|_| "凭据编码无效".into())
        }
    }
    fn delete_secret(&mut self, reference: &str) -> Result<(), String> {
        if !self.owns(reference) {
            return Err("凭据引用无效".into());
        }
        let target = util::wide(reference);
        unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) }
            .map_err(|_| "旧凭据清理失败".into())
    }
    fn commit(&mut self, cfg: &Settings) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| "配置锁不可用")?;
        if *self.expected.lock().unwrap() != Some(self.hash()?) {
            return Err("配置已被其他会话修改或尚不可读，请重新启动 BeeK 后再保存".into());
        }
        let temporary = self.path.with_extension(format!("{}.tmp", util::nonce()?));
        let result = (|| {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| "无法创建临时配置")?;
            let data = serde_json::to_vec_pretty(cfg).map_err(|_| "配置编码失败")?;
            f.write_all(&data)
                .and_then(|_| f.sync_all())
                .map_err(|_| "配置写入失败，原配置已保留")?;
            drop(f);
            let src = util::wide(&temporary);
            let dst = util::wide(&self.path);
            unsafe {
                if self.path.exists() {
                    ReplaceFileW(
                        PCWSTR(dst.as_ptr()),
                        PCWSTR(src.as_ptr()),
                        None,
                        REPLACE_FILE_FLAGS(0),
                        None,
                        None,
                    )
                } else {
                    MoveFileExW(
                        PCWSTR(src.as_ptr()),
                        PCWSTR(dst.as_ptr()),
                        MOVEFILE_WRITE_THROUGH,
                    )
                }
            }
            .map_err(|_| "配置原子替换失败，原配置已保留")?;
            *self.expected.lock().unwrap() = Some(Some(Sha256::digest(&data).into()));
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn parse(data: &[u8]) -> Result<Settings, serde_json::Error> {
    let value: serde_json::Value = serde_json::from_slice(data)?;
    if value.get("schema").is_some()
        || value.get("provider").is_some()
        || value.get("providers").is_none()
    {
        return serde_json::from_value(value);
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct LegacyArk {
        api_key: String,
        model: String,
        base_url: String,
    }
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct LegacyProviders {
        ark: Option<LegacyArk>,
    }
    #[derive(serde::Deserialize)]
    struct Legacy {
        providers: LegacyProviders,
    }
    let old: Legacy = serde_json::from_value(value)?;
    let mut cfg = Settings::default();
    if let Some(ark) = old.providers.ark {
        cfg.provider.api_key = ark.api_key;
        if !ark.model.is_empty() {
            cfg.provider.model = ark.model;
        }
        if !ark.base_url.is_empty() {
            cfg.provider.base_url = ark.base_url;
        }
    }
    Ok(cfg)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Store {
        let namespace = format!("BeeK/test-{}/ark/", util::nonce().unwrap());
        let path = std::env::temp_dir()
            .join(format!("beek-store-{}", util::nonce().unwrap()))
            .join("config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        Store {
            path,
            namespace,
            lock: Mutex::new(()),
            expected: Mutex::new(None),
        }
    }
    fn cleanup(mut s: Store) {
        let _ = fs::remove_file(&s.path);
        *s.expected.lock().unwrap() = Some(None);
        s.cleanup_older_than(std::time::Duration::ZERO).unwrap();
        fs::remove_dir_all(s.path.parent().unwrap()).unwrap();
    }
    #[test]
    #[ignore = "requires an interactive Windows logon; run --include-ignored on the desktop"]
    fn real_vault_atomic_rollback_and_stale_session() {
        let mut s = store();
        let initial = s.load().unwrap();
        let old = s
            .save(
                &initial,
                initial.clone(),
                KeyEdit::Replace("public-old-fixture".into()),
            )
            .unwrap();
        let old_ref = old.provider.credential_ref.as_ref().unwrap();
        assert_eq!(s.read_secret(old_ref).unwrap(), "public-old-fixture");
        let before = fs::read(&s.path).unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .open(&s.path)
            .unwrap();
        let mut next = old.clone();
        next.provider.model = "changed".into();
        assert!(s
            .save(&old, next, KeyEdit::Replace("public-new-fixture".into()))
            .is_err());
        assert_eq!(fs::read(&s.path).unwrap(), before);
        assert_eq!(s.read_secret(old_ref).unwrap(), "public-old-fixture");
        drop(held);
        let orphan = s.create_secret("public-orphan-fixture").unwrap();
        s.cleanup_older_than(std::time::Duration::ZERO).unwrap();
        assert!(s.read_secret(&orphan).is_err());
        assert_eq!(s.read_secret(old_ref).unwrap(), "public-old-fixture");
        assert!(s.delete_secret("another-application/credential").is_err());
        fs::write(&s.path, b"{}").unwrap();
        assert!(s.save(&old, old.clone(), KeyEdit::Keep).is_err());
        s.load().unwrap();
        cleanup(s);
    }
    #[test]
    fn legacy_types_and_future_schema_are_not_silently_downgraded() {
        assert!(parse(br#"{"providers":{}}"#).is_ok());
        assert!(parse(br#"{"providers":{"ark":{"api_key":42}}}"#).is_err());
        assert!(parse(br#"{"schema":2,"providers":{}}"#).is_err());
    }
}

```

## src/platform/windows/lifecycle.rs
```
use super::{
    native::Event,
    util::{self, Handle},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{BOOL, HRESULT, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::*, IO::*},
        UI::WindowsAndMessaging::AllowSetForegroundWindow,
    },
};

// Installed payloads live in immutable version directories. This shared read
// handle excludes an installer/uninstaller's exclusive lock for the app lifetime.
pub fn install_guard() -> Result<Option<fs::File>, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
    let dir = exe.parent().ok_or("程序目录不可用")?;
    if !dir.join("installed.marker").exists() {
        return Ok(None);
    }
    let root = dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|v| v == "versions"))
        .and_then(|p| p.parent())
        .ok_or("安装目录结构无效")?;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(root.join("install.lock"))
        .map(Some)
        .map_err(|_| "安装/卸载正在进行，请完成后重试".into())
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    pid: u32,
    created: u64,
    session: u32,
}
fn identity(pid: u32) -> Result<Identity, String> {
    unsafe {
        let p = Handle(
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
            .map_err(|_| "进程身份不可读")?,
        );
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u).map_err(|_| "进程创建时间不可读")?;
        let mut session = 0;
        ProcessIdToSessionId(pid, &mut session).map_err(|_| "进程会话不可读")?;
        Ok(Identity {
            pid,
            created: ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64,
            session,
        })
    }
}
fn me() -> Result<Identity, String> {
    identity(unsafe { GetCurrentProcessId() })
}
struct Security {
    descriptor: PSECURITY_DESCRIPTOR,
}
impl Security {
    fn new(sid: &str) -> Result<Self, String> {
        unsafe {
            let sddl = util::wide(format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(|_| "无法设置 IPC 访问控制")?;
            Ok(Self { descriptor })
        }
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: BOOL(0),
        }
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}
fn user_sid() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|_| "无法读取用户令牌")?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 || needed > 64 * 1024 {
            return Err("用户令牌尺寸无效".into());
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(bytes.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        )
        .map_err(|_| "无法读取用户 SID")?;
        let user = &*(bytes.as_ptr() as *const TOKEN_USER);
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(|_| "无法格式化用户 SID")?;
        let result = sid.to_string().map_err(|_| "用户 SID 编码错误".to_owned());
        let _ = LocalFree(Some(HLOCAL(sid.0 as *mut _)));
        result
    }
}
struct Pipe(Handle);
impl Pipe {
    fn server(name: &str, sid: &str) -> Result<Self, String> {
        let security = Security::new(sid)?;
        let name = util::wide(name);
        let attributes = security.attributes();
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                1000,
                Some(&attributes),
            )
        };
        if h.is_invalid() {
            return Err("无法创建受保护的本地管道".into());
        }
        Ok(Self(Handle(h)))
    }
    fn client(name: &str) -> Result<Self, String> {
        let name = util::wide(name);
        let h = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                0xc0000000,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|_| "无法连接正在运行的 BeeK")?;
        Ok(Self(Handle(h)))
    }
    fn connect(&self, timeout: u32) -> Result<(), String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = ConnectNamedPipe(self.0 .0, Some(&mut ov));
            if status
                .as_ref()
                .is_err_and(|e| e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0))
            {
                return Ok(());
            }
            complete(self.0 .0, &mut ov, status, timeout).map(|_| ())
        }
    }
    fn read<T: for<'a> Deserialize<'a>>(&self) -> Result<T, String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let mut bytes = [0u8; 4097];
            let status = ReadFile(self.0 .0, Some(&mut bytes), None, Some(&mut ov));
            let n = complete(self.0 .0, &mut ov, status, 1000)?;
            if n == 0 || n > 4096 {
                return Err("IPC 消息长度无效".into());
            }
            serde_json::from_slice(&bytes[..n]).map_err(|_| "IPC 消息格式无效".into())
        }
    }
    fn write(&self, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec(value).map_err(|_| "IPC 消息编码失败")?;
        if bytes.len() > 4096 {
            return Err("IPC 消息过长".into());
        }
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = WriteFile(self.0 .0, Some(&bytes), None, Some(&mut ov));
            if complete(self.0 .0, &mut ov, status, 1000)? != bytes.len() {
                return Err("IPC 消息未完整发送".into());
            }
        }
        Ok(())
    }
    fn peer(&self, server: bool) -> Result<Identity, String> {
        let mut pid = 0;
        unsafe {
            if server {
                GetNamedPipeServerProcessId(self.0 .0, &mut pid)
            } else {
                GetNamedPipeClientProcessId(self.0 .0, &mut pid)
            }
        }
        .map_err(|_| "无法核验管道对端进程")?;
        identity(pid)
    }
}
unsafe fn complete(
    h: HANDLE,
    ov: &mut OVERLAPPED,
    status: windows::core::Result<()>,
    timeout: u32,
) -> Result<usize, String> {
    unsafe {
        if let Err(e) = status {
            if e.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0) {
                return Err("IPC 操作失败".into());
            }
        }
        if WaitForSingleObject(ov.hEvent, timeout) != WAIT_OBJECT_0 {
            let _ = CancelIoEx(h, Some(ov));
            let mut done = 0;
            let _ = GetOverlappedResult(h, ov, &mut done, true);
            return Err("IPC 操作超时".into());
        }
        let mut n = 0;
        GetOverlappedResult(h, ov, &mut n, false).map_err(|_| "IPC 操作中断")?;
        Ok(n as usize)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    version: u32,
    identity: Identity,
    nonce: String,
    exe: PathBuf,
}
#[derive(Serialize, Deserialize)]
enum PublicCommand {
    Ping,
    ActivateSettings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicRequest {
    version: u32,
    nonce: String,
    client: Identity,
    command: PublicCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicReply {
    version: u32,
    nonce: String,
    server: Identity,
    ok: bool,
}

pub struct Instance {
    mutex: Handle,
    _running: Handle,
    stop: Arc<AtomicBool>,
    listener: Option<thread::JoinHandle<()>>,
    pub restart_events: Option<mpsc::Receiver<RestartEvent>>,
    grant: Option<mpsc::SyncSender<()>>,
    sid: String,
    identity: Identity,
}
impl Instance {
    pub fn begin(events: mpsc::SyncSender<Event>) -> Result<Option<Self>, String> {
        let sid = user_sid()?;
        let own = me()?;
        let security = Security::new(&sid)?;
        let attributes = security.attributes();
        let name = util::wide(format!("Local\\BeeK-{sid}-{}", own.session));
        let mutex = unsafe { CreateMutexW(Some(&attributes), true, PCWSTR(name.as_ptr())) }
            .map_err(|_| "无法取得 BeeK 会话锁")?;
        let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mutex = Handle(mutex);
        let pipe_name = format!("\\\\.\\pipe\\BeeK-{sid}-{}", own.session);
        let path = util::known_folder(true)?.join(format!("instance-{}.json", own.session));
        if existing {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Ok(bytes) = fs::read(&path) {
                    if bytes.len() <= 4096 {
                        if let Ok(owner) = serde_json::from_slice::<Owner>(&bytes) {
                            if owner.version == 1
                                && identity(owner.identity.pid).ok() == Some(owner.identity)
                                && owner.identity.session == own.session
                            {
                                if let Ok(pipe) = Pipe::client(&pipe_name) {
                                    if pipe.peer(true)? != owner.identity {
                                        return Err("BeeK 管道服务身份不匹配".into());
                                    }
                                    unsafe {
                                        let _ = AllowSetForegroundWindow(owner.identity.pid);
                                    }
                                    pipe.write(&PublicRequest {
                                        version: 1,
                                        nonce: owner.nonce.clone(),
                                        client: own,
                                        command: PublicCommand::ActivateSettings,
                                    })?;
                                    let response: PublicReply = pipe.read()?;
                                    if response.version == 1
                                        && response.nonce == owner.nonce
                                        && response.server == owner.identity
                                        && response.ok
                                    {
                                        return Ok(None);
                                    }
                                    return Err("BeeK 会话握手失败".into());
                                }
                            }
                        }
                    }
                }
                if Instant::now() >= deadline {
                    return Err("BeeK 已在启动或暂时无响应，请稍后重试".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建会话目录")?;
        let owner = Owner {
            version: 1,
            identity: own,
            nonce: util::nonce()?,
            exe: std::env::current_exe().map_err(|_| "程序路径不可用")?,
        };
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec(&owner).unwrap())
            .map_err(|_| "会话记录写入失败")?;
        if path.exists() {
            fs::remove_file(&path).map_err(|_| "旧会话记录无法清理")?;
        }
        fs::rename(temporary, &path).map_err(|_| "会话记录提交失败")?;
        // Create the first pipe before reporting readiness. Each connection uses
        // FIRST_PIPE_INSTANCE and verifies the peer PID, creation time and session.
        let first = Pipe::server(&pipe_name, &sid)?;
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let thread_sid = sid.clone();
        let listener = thread::spawn(move || {
            let mut next = Some(first);
            while !shutdown.load(Ordering::Acquire) {
                let pipe = match next
                    .take()
                    .or_else(|| Pipe::server(&pipe_name, &thread_sid).ok())
                {
                    Some(p) => p,
                    None => {
                        thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                };
                if pipe.connect(250).is_err() {
                    continue;
                }
                let request = pipe.read::<PublicRequest>();
                if let Ok(request) = request {
                    let valid = request.version == 1
                        && request.nonce == owner.nonce
                        && request.client.session == own.session
                        && pipe.peer(false).ok() == Some(request.client);
                    if valid {
                        if matches!(request.command, PublicCommand::ActivateSettings) {
                            let _ = events.try_send(Event::Settings);
                        }
                        let _ = pipe.write(&PublicReply {
                            version: 1,
                            nonce: owner.nonce.clone(),
                            server: own,
                            ok: true,
                        });
                    }
                }
            }
        });
        let running = Handle(
            unsafe {
                CreateMutexW(
                    Some(&attributes),
                    true,
                    windows::core::w!("Local\\BeeK-Running"),
                )
            }
            .map_err(|_| "无法创建安装协作锁")?,
        );
        Ok(Some(Self {
            mutex,
            _running: running,
            stop,
            listener: Some(listener),
            restart_events: None,
            grant: None,
            sid,
            identity: own,
        }))
    }
    pub fn restart(&mut self) -> Result<(), String> {
        if self.restart_events.is_some() {
            return Err("重启正在进行".into());
        }
        let nonce = util::nonce()?;
        let name = format!(
            "\\\\.\\pipe\\BeeK-restart-{}-{}-{nonce}",
            self.sid, self.identity.session
        );
        let pipe = Pipe::server(&name, &self.sid)?;
        let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
        let mut child = std::process::Command::new(exe)
            .args([
                "--restart-child",
                &self.identity.pid.to_string(),
                &self.identity.created.to_string(),
                &nonce,
            ])
            .spawn()
            .map_err(|_| "无法启动重启子进程，旧进程继续运行")?;
        let child_pid = child.id();
        let (tx, rx) = mpsc::sync_channel(2);
        let (grant_tx, grant_rx) = mpsc::sync_channel(1);
        let parent = self.identity;
        thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                pipe.connect(5000)?;
                let message: RestartMessage = pipe.read()?;
                let actual = pipe.peer(false)?;
                if message.version != 1
                    || message.nonce != nonce
                    || message.process != actual
                    || actual.pid != child_pid
                    || actual.session != parent.session
                    || message.state != RestartState::ChildReady
                {
                    return Err("重启子进程未就绪，旧进程继续运行".into());
                }
                tx.send(RestartEvent::Ready).map_err(|_| "重启已取消")?;
                grant_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "重启提交超时")?;
                pipe.write(&RestartMessage {
                    version: 1,
                    nonce: nonce.clone(),
                    process: parent,
                    state: RestartState::ParentExitCommitted,
                })?;
                let accepted: RestartMessage = pipe.read()?;
                if accepted.version != 1
                    || accepted.nonce != nonce
                    || accepted.process != actual
                    || accepted.state != RestartState::ChildCommitAccepted
                {
                    return Err("重启提交未确认，旧进程继续运行".into());
                }
                let _ = tx.send(RestartEvent::Committed);
                Ok(())
            })();
            if let Err(e) = result {
                let _ = child.kill();
                let _ = child.wait();
                let _ = tx.send(RestartEvent::Failed(e));
            }
        });
        self.restart_events = Some(rx);
        self.grant = Some(grant_tx);
        Ok(())
    }
    pub fn grant_restart(&mut self) {
        if let Some(tx) = self.grant.take() {
            let _ = tx.try_send(());
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let at = Instant::now();
        while self.listener.as_ref().is_some_and(|t| !t.is_finished())
            && at.elapsed() < Duration::from_millis(1100)
        {
            thread::sleep(Duration::from_millis(5));
        }
        if self.listener.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = self.listener.take().unwrap().join();
        }
        unsafe {
            let _ = ReleaseMutex(self.mutex.0);
        }
    }
}
pub enum RestartEvent {
    Ready,
    Committed,
    Failed(String),
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
enum RestartState {
    ChildReady,
    ParentExitCommitted,
    ChildCommitAccepted,
    RestartFailed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartMessage {
    version: u32,
    nonce: String,
    process: Identity,
    state: RestartState,
}

pub fn child_preflight() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--restart-child") {
        return Ok(());
    }
    if args.len() != 5 || args[4].len() != 48 || !args[4].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("重启参数无效".into());
    }
    let pid = args[2].parse().map_err(|_| "重启父进程无效")?;
    let created: u64 = args[3].parse().map_err(|_| "重启父进程创建时间无效")?;
    let parent = identity(pid)?;
    let own = me()?;
    if parent.created != created || parent.session != own.session {
        return Err("重启父进程身份不匹配".into());
    }
    super::pair::helper()?;
    super::store::Store::open()?.load()?;
    let sid = user_sid()?;
    let pipe = Pipe::client(&format!(
        "\\\\.\\pipe\\BeeK-restart-{sid}-{}-{}",
        own.session, args[4]
    ))?;
    if pipe.peer(true)? != parent {
        return Err("重启管道父进程身份无效".into());
    }
    let parent_handle = Handle(
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .map_err(|_| "重启父进程不可读")?,
    );
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildReady,
    })?;
    let response: RestartMessage = pipe.read()?;
    if response.version != 1
        || response.nonce != args[4]
        || response.process != parent
        || response.state != RestartState::ParentExitCommitted
    {
        return Err("重启未得到父进程提交".into());
    }
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildCommitAccepted,
    })?;
    if unsafe { WaitForSingleObject(parent_handle.0, 10_000) } != WAIT_OBJECT_0 {
        return Err("旧进程未按时退出，子进程已取消启动".into());
    }
    Ok(())
}
pub fn exit_deadline() {
    // Only bounds an intentional shutdown; never restarts a crash.
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(1900));
        std::process::exit(0);
    });
}

```

## src/platform/windows/pair.rs
```
use windows::{core::*, Win32::Storage::FileSystem::*};
pub fn helper() -> std::result::Result<std::path::PathBuf, String> {
    let helper = std::env::current_exe()
        .map_err(|_| "无法定位 BeeK 程序")?
        .with_file_name("beek-selection-host.exe");
    let path = super::util::wide(&helper);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 || size > 1024 * 1024 {
            return Err("选区助手缺失或版本信息无效，请重新安装完整 BeeK".into());
        }
        let mut bytes = vec![0u8; size as usize];
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            size,
            bytes.as_mut_ptr() as *mut _,
        )
        .map_err(|_| "无法读取选区助手版本")?;
        let mut ptr = std::ptr::null_mut();
        let mut length = 0;
        if !VerQueryValueW(
            bytes.as_ptr() as *const _,
            w!("\\StringFileInfo\\040904b0\\BeeKBuildId"),
            &mut ptr,
            &mut length,
        )
        .as_bool()
            || ptr.is_null()
            || length != 65
        {
            return Err("选区助手与主程序不配套，请重新安装完整 BeeK".into());
        }
        let value = String::from_utf16(std::slice::from_raw_parts(ptr as *const u16, 64))
            .map_err(|_| "助手版本信息编码无效")?;
        if value != env!("BEEK_BUILD_ID") {
            return Err("选区助手与主程序构建不一致，请重新安装完整 BeeK".into());
        }
    }
    Ok(helper)
}

```
