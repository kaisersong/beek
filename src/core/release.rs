//! Exact stable Windows asset selection; returned URLs are never executed.
use serde::Deserialize;
use std::{io::Read, time::Duration};

const REPOSITORY: &str = "https://github.com/kaisersong/beek";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

/// Use the public latest-release redirect instead of the shared anonymous REST
/// quota. Do not follow redirects or trust download links outside this repo.
pub fn check_windows_update(current: &str) -> Result<Option<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("BeeK/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法初始化更新检查")?;
    let response = client
        .head(format!("{REPOSITORY}/releases/latest"))
        .send()
        .map_err(|_| "更新检查失败，请稍后重试")?;
    if response.status() != reqwest::StatusCode::FOUND {
        return Err(format!(
            "更新检查失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or("更新信息格式无效")?;
    let mut release = release_from_location(location)?;
    // No asset request is needed when the installed version is already current.
    if matches!(windows_update(&release, current), Ok(None)) {
        return Ok(None);
    }
    let response = client
        .get(format!(
            "{REPOSITORY}/releases/expanded_assets/{}",
            release.tag_name
        ))
        .send()
        .map_err(|_| "更新检查失败，请稍后重试")?;
    if !response.status().is_success() {
        return Err(format!(
            "更新检查失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let html = read_assets(response)?;
    release.assets = assets_from_html(&html);
    windows_update(&release, current)
}

fn release_from_location(location: &str) -> Result<Release, String> {
    let tag = location
        .strip_prefix(&format!("{REPOSITORY}/releases/tag/v"))
        .ok_or("更新信息格式无效")?;
    let parsed = version(tag).ok_or("发布版本格式无效")?;
    if tag != format!("{}.{}.{}", parsed[0], parsed[1], parsed[2]) {
        return Err("发布版本格式无效".into());
    }
    Ok(Release {
        tag_name: format!("v{tag}"),
        draft: false,
        prerelease: false,
        assets: Vec::new(),
    })
}

fn read_assets(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "更新响应读取失败")?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("更新响应超出限制".into());
    }
    String::from_utf8(bytes).map_err(|_| "更新信息格式无效".into())
}

fn assets_from_html(html: &str) -> Vec<Asset> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|tail| {
            let href = tail.split_once('"')?.0;
            let path = href.strip_prefix("/kaisersong/beek/releases/download/")?;
            let name = path.rsplit_once('/')?.1;
            Some(Asset {
                name: name.into(),
                browser_download_url: format!("{REPOSITORY}/releases/download/{path}"),
            })
        })
        .collect()
}
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
    fn public_release_discovery_keeps_stable_repo_and_asset_checks() {
        for location in [
            "https://other.invalid/kaisersong/beek/releases/tag/v1.1.2",
            "https://github.com/other/beek/releases/tag/v1.1.2",
            "http://github.com/kaisersong/beek/releases/tag/v1.1.2",
            "https://github.com/kaisersong/beek/releases/tag/v1.1.2-beta",
            "https://github.com/kaisersong/beek/releases/tag/v1.1.2?extra",
            "https://github.com/kaisersong/beek/releases/tag/v01.1.2",
        ] {
            assert!(release_from_location(location).is_err(), "{location}");
        }
        let mut release =
            release_from_location(&format!("{REPOSITORY}/releases/tag/v1.1.2")).unwrap();
        assert_eq!(windows_update(&release, "1.1.2").unwrap(), None);
        assert_eq!(windows_update(&release, "1.1.3").unwrap(), None);
        release.assets = assets_from_html(
            r#"<a href="/kaisersong/beek/releases/download/v1.1.2/BeeK-v1.1.2-windows-x64.zip">ZIP</a>
            <a href="https://other.invalid/BeeK-v1.1.2-windows-x64-setup.exe">EXE</a>
            <a href="/kaisersong/beek/releases/download/v1.1.1/BeeK-v1.1.2-windows-x64-setup.exe">old</a>"#,
        );
        assert!(windows_update(&release, "1.1.1").is_err());
        release.assets.extend(assets_from_html(
            r#"<a href="/kaisersong/beek/releases/download/v1.1.2/BeeK-v1.1.2-windows-x64-setup.exe">EXE</a>"#,
        ));
        assert_eq!(
            windows_update(&release, "1.1.1").unwrap(),
            Some("1.1.2".into())
        );
        assert_eq!(
            windows_update(&release, "1.1.2-beta").unwrap(),
            Some("1.1.2".into())
        );
    }

    #[test]
    fn public_release_response_is_bounded_and_valid_utf8() {
        assert!(read_assets(&vec![b'x'; MAX_RESPONSE_BYTES as usize][..]).is_ok());
        assert!(read_assets(&vec![b'x'; MAX_RESPONSE_BYTES as usize + 1][..]).is_err());
        assert!(read_assets(&b"\xff"[..]).is_err());
    }

    #[test]
    #[ignore = "explicit live GitHub check; no anonymous REST API requests"]
    fn live_public_windows_update() {
        let update = check_windows_update("0.0.0").unwrap().unwrap();
        assert!(version(&update).is_some());
        assert_eq!(check_windows_update(&update).unwrap(), None);
        println!("Public Windows update discovery: v{update}; current-version check passed");
    }
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
