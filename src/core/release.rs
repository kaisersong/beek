//! Exact stable Windows asset selection; returned URLs are never executed.
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}
pub fn version(value: &str) -> Option<[u32; 3]> {
    let v = value.strip_prefix('v').unwrap_or(value);
    let items = v.split('.').collect::<Vec<_>>();
    if items.len() != 3 {
        return None;
    }
    let mut out = [0; 3];
    for (i, v) in items.iter().enumerate() {
        if v.is_empty() || !v.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out[i] = v.parse().ok()?;
    }
    Some(out)
}
pub fn windows_update(release: &Release, current: &str) -> Result<Option<String>, String> {
    if release.draft || release.prerelease {
        return Err("该版本不是稳定发布".into());
    }
    let latest = version(&release.tag_name).ok_or("发布版本格式无效")?;
    let prerelease = current.contains('-');
    let current =
        version(current.split(['-', '+']).next().unwrap_or(current)).ok_or("当前版本格式无效")?;
    if latest < current || (latest == current && !prerelease) {
        return Ok(None);
    }
    let number = format!("{}.{}.{}", latest[0], latest[1], latest[2]);
    for suffix in ["windows-x64.zip", "windows-x64-setup.exe"] {
        let name = format!("BeeK-v{number}-{suffix}");
        let url = format!("https://github.com/kaisersong/beek/releases/download/v{number}/{name}");
        if !release
            .assets
            .iter()
            .any(|a| a.name == name && a.browser_download_url == url)
        {
            return Err(format!("版本 {number} 暂无完整 Windows x64 安装包"));
        }
    }
    Ok(Some(number))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_platform_channel_and_urls() {
        let mut r = Release {
            tag_name: "v1.0.15".into(),
            draft: false,
            prerelease: false,
            assets: vec![],
        };
        assert!(windows_update(&r, "1.0.14").is_err());
        for suffix in ["windows-x64.zip", "windows-x64-setup.exe"] {
            let name = format!("BeeK-v1.0.15-{suffix}");
            r.assets.push(Asset {
                browser_download_url: format!(
                    "https://github.com/kaisersong/beek/releases/download/v1.0.15/{name}"
                ),
                name,
            });
        }
        assert_eq!(windows_update(&r, "1.0.14").unwrap(), Some("1.0.15".into()));
        r.assets[1].browser_download_url = "https://other.invalid/setup.exe".into();
        assert!(windows_update(&r, "1.0.14").is_err());
        r.prerelease = true;
        assert!(windows_update(&r, "1.0.14").is_err());
        assert!(version("1.0.15-beta").is_none());
    }
}
