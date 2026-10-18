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
        let backup = self
            .hash()
            .and_then(|hash| hash.ok_or("配置已消失".into()))
            .map(|hash| {
                self.path
                    .with_extension(format!("corrupt-{:x}.json", Sha256::digest(hash)))
            });
        if backup
            .and_then(|path| {
                if path.exists() {
                    Ok(0)
                } else {
                    fs::copy(&self.path, path).map_err(|_| "备份失败".into())
                }
            })
            .is_err()
        {
            *self.expected.lock().unwrap() = None;
            format!("{reason}；无法备份原文件，请手动恢复后重新启动")
        } else {
            self.prune_backups();
            format!("{reason}；原文件已备份，保存恢复配置前请确认设置")
        }
    }
    fn prune_backups(&self) {
        if let Some(dir) = self.path.parent() {
            if let Ok(entries) = fs::read_dir(dir) {
                let mut backups = entries
                    .flatten()
                    .filter(|e| {
                        let name = e.file_name();
                        let n = name.to_string_lossy();
                        n.strip_prefix("config.corrupt-")
                            .and_then(|n| n.strip_suffix(".json"))
                            .is_some_and(|n| {
                                matches!(n.len(), 48 | 64)
                                    && n.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                    })
                    .filter_map(|e| {
                        e.metadata()
                            .ok()
                            .filter(|m| m.is_file())
                            .and_then(|m| m.modified().ok())
                            .map(|t| (t, e.path()))
                    })
                    .collect::<Vec<_>>();
                backups.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
                for (t, path) in backups.into_iter().skip(3) {
                    if t.elapsed()
                        .is_ok_and(|age| age >= std::time::Duration::from_secs(24 * 3600))
                    {
                        let _ = fs::remove_file(path);
                    }
                }
            }
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
