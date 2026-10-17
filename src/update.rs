// BeeK 更新模块 — 查 GitHub Releases → 下载 zip → 替换 /Applications/BeeK.app
//
// 为什么走 GitHub Releases 而不是自己搭更新源：仓库本来就是公开的，
// 发布一个新版本 = 打 tag + 传一个 zip，用户的 app 自己去查，不需要服务器。
//
// 替换为什么交给一个临时 shell 脚本：正在运行的 app 不能删掉自己再把自己拷回来
// （bundle 被占用），所以流程是「先把新 app 解压好 → 生成脚本 → 退出自己 →
// 脚本等进程退出后替换 + 重新签名 + 让 launchd 重新拉起」。
// 这里只做纯逻辑和 IO，弹窗由主循环负责（AppKit 只能在主线程用）。
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// GitHub 仓库（检查更新 / 打开 README / 发布页都用它）
pub const REPO: &str = "kaisersong/beek";
pub const README_URL: &str = "https://github.com/kaisersong/beek#readme";
/// README 里「获取 API Key」那一节的锚点（GitHub 按标题自动生成）
pub const README_KEY_URL: &str = "https://github.com/kaisersong/beek#获取-api-key";
pub const RELEASES_URL: &str = "https://github.com/kaisersong/beek/releases";
/// 发布信息用 Atom feed 取，不用 GitHub REST API：
/// API 匿名配额是按出口 IP 算的（60 次/小时），实测本机出口 IP 经常已经用光，
/// 直接返回 403 rate limit；Atom feed 不需要 token 也没有这个限制。
const RELEASES_ATOM: &str = "https://github.com/kaisersong/beek/releases.atom";
/// 升级包地址：GitHub 的「最新版资产」直链，按资产名推导，不需要 API
const LATEST_DOWNLOAD: &str = "https://github.com/kaisersong/beek/releases/latest/download";

/// 当前版本：编译期写进二进制的 Cargo 版本，是「有没有新版本」的基准
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// launchd 的 label（替换完成后用它把 app 再拉起来）
pub const LAUNCH_LABEL: &str = "com.kaisersong.beek";

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// 去掉 v 前缀的版本号，例如 "1.2.0"
    pub version: String,
    /// 原始 tag，例如 "v1.2.0"
    pub tag: String,
    pub notes: String,
    pub page_url: String,
    /// 升级包下载地址（BeeK-vX.Y.Z.zip）；没有这个资产时为 None
    pub asset_url: Option<String>,
}


/// 版本号解析：容忍 v 前缀、缺位（1.2 → 1.2.0）、以及 -rc1 / +build 后缀。
/// 解析不出来时返回 None —— 调用方一律按「不新」处理，绝不因为解析失败就提示升级。
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let t = s.trim().trim_start_matches(['v', 'V']);
    let core = t.split(['-', '+']).next().unwrap_or("").trim();
    if core.is_empty() {
        return None;
    }
    let mut it = core.split('.');
    let major: u64 = it.next()?.trim().parse().ok()?;
    let minor: u64 = it.next().unwrap_or("0").trim().parse().unwrap_or(0);
    let patch: u64 = it.next().unwrap_or("0").trim().parse().unwrap_or(0);
    Some((major, minor, patch))
}

/// 远端版本是否比当前版本新
pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

/// 约定：升级包资产名 = BeeK-v<版本>.zip
pub fn asset_name(version: &str) -> String {
    format!("BeeK-v{}.zip", version)
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        // GitHub API 强制要求 User-Agent
        .user_agent(format!("BeeK/{}", CURRENT_VERSION))
        .build()
        .map_err(|e| format!("HTTP 客户端创建失败：{}", e))
}

/// 取出 XML 里一对标签之间的内容（够用就行：feed 的结构非常固定，不值得引 XML 库）
fn between(hay: &str, start: &str, end: &str) -> Option<String> {
    let i = hay.find(start)? + start.len();
    let j = hay[i..].find(end)? + i;
    Some(hay[i..j].to_string())
}

/// 取 `<a ... href="X">` 里的 X：先找到锚点，再找它后面的 href
fn href_after(hay: &str, anchor: &str) -> Option<String> {
    let i = hay.find(anchor)?;
    let rest = &hay[i..];
    let k = rest.find("href=\"")? + 6;
    let e = rest[k..].find('"')? + k;
    Some(rest[k..e].to_string())
}

/// 把 release 说明里的 HTML 粗粗转成纯文本（GitHub 的 atom 里 content 是 HTML）
fn strip_html(html: &str) -> String {
    // 必须先反转义、再删标签：atom 里的正文是**转义过的 HTML**（&lt;h2&gt;…），
    // 先删标签的话删不到，反转义后标签就露出来了（实测踩过）
    let unescaped = html
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut out = String::new();
    let mut in_tag = false;
    for c in unescaped.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(12)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 升级包能不能下：对约定的资产地址做一次 HEAD。
/// 200 → 可以自动更新；404/其他 → 只能手动下载（也会在弹窗里说明）。
fn probe_asset(version: &str) -> Option<String> {
    let url = format!("{}/{}", LATEST_DOWNLOAD, asset_name(version));
    match client().ok()?.head(&url).send() {
        Ok(r) if r.status().is_success() => Some(url),
        Ok(r) => {
            eprintln!("[beek] 升级包探测：HTTP {}（{}）", r.status(), url);
            None
        }
        Err(e) => {
            // 探测失败 ≠ 资产不存在：atom 都拿到了，说明网络是通的，多半是这一次 HEAD 抽风。
            // 这时**不能**报告「只能手动下载」——那会把整条自动更新路悄悄关掉。
            // 仍然按约定地址返回，真的没有这个资产时下载那步会报明确的 HTTP 错误。
            eprintln!("[beek] 升级包探测没成功（{}），仍按约定地址尝试：{}", e, url);
            Some(url)
        }
    }
}

/// 查最新 release。返回 Ok(None) 表示「没有发布过任何 release」（还没发过版，不算错误）。
/// 预发布版（tag 里带 -beta 之类）会被跳过，避免稳定版用户被推去装 beta。
pub fn check() -> Result<Option<Release>, String> {
    let resp = client()?
        .get(RELEASES_ATOM)
        .send()
        .map_err(|e| format!("请求失败：{}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}（拿不到发布信息）", resp.status()));
    }
    let xml = resp.text().map_err(|e| format!("读取响应失败：{}", e))?;

    let mut rest = xml.as_str();
    while let Some(i) = rest.find("<entry>") {
        let tail = &rest[i..];
        let entry = match tail.find("</entry>") {
            Some(j) => &tail[..j + "</entry>".len()],
            None => tail,
        };
        rest = &tail[entry.len().min(tail.len())..];

        // 版本号要从 link 里的 tag 取：GitHub 的 atom 里 <title> 是**发布标题**
        // （我们发布时写的是 "BeeK v1.0.0"），不是 tag。实测踩过这个坑：
        // 拿标题当版本号会解析失败，于是整条 release 被当成「无法识别的 tag」跳过。
        let page_url = href_after(entry, "rel=\"alternate\"").unwrap_or_default();
        let tag = page_url
            .split("/releases/tag/")
            .nth(1)
            .map(|t| t.trim_matches('/').to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| between(entry, "<title>", "</title>").map(|t| t.trim().to_string()));
        let tag = match tag {
            Some(t) => t,
            None => continue,
        };
        let version = tag.trim_start_matches(['v', 'V']).to_string();
        // 预发布版跳过：能解析出三段版本号、且不含 - / + 后缀
        if parse_version(&tag).is_none() || version.contains('-') || version.contains('+') {
            eprintln!("[beek] 跳过预发布/无法识别的 tag：{}", tag);
            continue;
        }
        let page_url = if page_url.is_empty() {
            format!("{}/{}", RELEASES_URL, tag)
        } else {
            page_url
        };
        let notes = between(entry, "<content type=\"html\">", "</content>")
            .map(|c| strip_html(&c))
            .unwrap_or_default();
        let asset_url = probe_asset(&version);
        return Ok(Some(Release {
            version,
            tag,
            notes,
            page_url,
            asset_url,
        }));
    }
    Ok(None)
}

/// 已下载待安装的更新（写进 update/staged.json）
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Staged {
    pub version: String,
    pub tag: String,
    pub app: String,
}

fn staged_path() -> PathBuf {
    update_dir().join("staged.json")
}

/// 后台下载完成 → 暂存下来，等退出或下次启动时装（对应 electron-updater 的 autoDownload）
pub fn stage(rel: &Release, app: &Path) -> Result<(), String> {
    std::fs::create_dir_all(update_dir()).map_err(|e| format!("创建更新目录失败：{}", e))?;
    let staged = Staged {
        version: rel.version.clone(),
        tag: rel.tag.clone(),
        app: app.display().to_string(),
    };
    let body = serde_json::to_string_pretty(&staged).map_err(|e| e.to_string())?;
    std::fs::write(staged_path(), body).map_err(|e| format!("写暂存标记失败：{}", e))
}

/// 读一眼暂存的更新（不消耗）
pub fn staged() -> Option<Staged> {
    let body = std::fs::read_to_string(staged_path()).ok()?;
    let staged: Staged = serde_json::from_str(&body).ok()?;
    // 版本比当前旧/相同就不算（比如手动装过更新的版本，标记还没清）
    if !is_newer(&staged.version, CURRENT_VERSION) {
        let _ = std::fs::remove_file(staged_path());
        return None;
    }
    // 解压出来的 app 还在吗（用户可能手动清了）
    if !PathBuf::from(&staged.app).is_dir() {
        let _ = std::fs::remove_file(staged_path());
        return None;
    }
    Some(staged)
}

pub fn clear_staged() {
    let _ = std::fs::remove_file(staged_path());
}

/// 更新包存放目录：~/Library/Application Support/BeeK/update
pub fn update_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/BeeK/update")
}

/// 下载 zip 并解压，返回解压出来的 .app 路径。只做准备，不碰正在运行的 app。
pub fn prepare(rel: &Release) -> Result<PathBuf, String> {
    let url = rel
        .asset_url
        .clone()
        .ok_or_else(|| format!("这条 release 里没有 {} 资产", asset_name(&rel.version)))?;
    let dir = update_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建更新目录失败：{}", e))?;
    let zip = dir.join(asset_name(&rel.version));

    let mut resp = client()?
        .get(&url)
        .send()
        .map_err(|e| format!("下载失败：{}", e))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status()));
    }
    let mut f = std::fs::File::create(&zip).map_err(|e| format!("写文件失败：{}", e))?;
    resp.copy_to(&mut f)
        .map_err(|e| format!("写升级包失败：{}", e))?;
    let _ = f.flush();
    let size = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
    eprintln!(
        "[beek] 升级包已下载：{}（{} 字节）",
        zip.display(),
        size
    );

    let unpacked = dir.join("unpacked");
    let _ = std::fs::remove_dir_all(&unpacked);
    let out = std::process::Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(&zip)
        .arg(&unpacked)
        .output()
        .map_err(|e| format!("解压失败（ditto 不可用）：{}", e))?;
    if !out.status.success() {
        return Err(format!(
            "解压失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    find_app(&unpacked).ok_or_else(|| "升级包里没有找到 .app".to_string())
}

/// 在解压目录里找 .app（zip 里可能直接是 BeeK.app，也可能套了一层目录）
fn find_app(dir: &Path) -> Option<PathBuf> {
    if dir.extension().map(|e| e == "app").unwrap_or(false) && dir.is_dir() {
        return Some(dir.to_path_buf());
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    if let Some(app) = entries
        .iter()
        .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
    {
        return Some(app.clone());
    }
    for sub in entries.iter().filter(|p| p.is_dir()) {
        if let Some(app) = find_app(sub) {
            return Some(app);
        }
    }
    None
}

/// 生成替换脚本（不改动 app，只写文件），返回脚本路径。
///
/// 脚本内容：让 launchd 先别拉起 → 等旧进程退出 → 换 bundle → 重新签名 → 再拉起。
/// 重新签名沿用安装脚本同样的策略（优先 Apple Development 证书）：
/// TCC 按签名身份记辅助功能授权，换成 ad-hoc 就会让用户重新授权一次。
pub fn install_script(new_app: &Path, version: &str) -> Result<PathBuf, String> {
    install_script_ex(new_app, version, true)
}

/// `relaunch=false` 时不重新拉起（退出时静默安装用：下次登录/启动自然是新版本）
pub fn install_script_ex(new_app: &Path, version: &str, relaunch: bool) -> Result<PathBuf, String> {
    install_script_to(new_app, version, relaunch, "beek_update")
}

/// 同 install_script_ex，但可以指定脚本名（诊断用：一次生成两种模式来对比）
pub fn install_script_to(
    new_app: &Path,
    version: &str,
    relaunch: bool,
    name: &str,
) -> Result<PathBuf, String> {
    let script = PathBuf::from(format!("/tmp/{}.sh", name));
    let body = format!(
        r#"#!/bin/sh
# 由 BeeK 自动更新生成：替换 /Applications/BeeK.app
LOG=/tmp/beek_update.log
exec >>"$LOG" 2>&1
echo "== $(date '+%F %T') 开始替换：{version} =="
APP="/Applications/BeeK.app"
# 路径必须加引号：解压目录在 ~/Library/Application Support 下，含空格，
# 不加引号会被 shell 截断（实测：脚本走到 ditto 时源路径为空、替换失败，旧 app 已被删）
NEW="{new_app}"
LABEL={label}
if [ "{relaunch}" = "1" ]; then
    # 立即更新：先让旧进程退出，再替换，最后重新拉起
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null
    i=0
    while pgrep -x BeeK >/dev/null && [ $i -lt 30 ]; do sleep 0.5; i=$((i+1)); done
    if pgrep -x BeeK >/dev/null; then echo "旧进程仍在运行，放弃替换"; exit 1; fi
else
    # 静默安装（退出时 / 下次启动时）：**不动进程**。
    # macOS 允许替换正在运行的 bundle（进程持有的是旧 inode），
    # 这里 bootout 的话会把用户正在用的 app 直接杀掉，得不偿失。
    echo "静默安装：只替换 bundle，不动正在运行的进程"
fi
rm -rf "$APP" || {{ echo "删除旧 app 失败"; exit 1; }}
ditto "$NEW" "$APP" || {{ echo "复制新 app 失败"; exit 1; }}
xattr -dr com.apple.quarantine "$APP" 2>/dev/null
# 如果新包本来就是 Developer ID 签名（CI 出的、已公证），就**保留原签名**：
# 重签会换成这台机器的 Apple Development 身份 → 辅助功能/自动化授权失效、
# 而且公证 staple 也跟着失效。只有本地/临时包才需要重签。
INCOMING_ID=$(codesign -dv --verbose=4 "$NEW" 2>&1 | sed -n 's/^Authority=\(Developer ID Application.*\)/\1/p' | head -1)
if [ -n "$INCOMING_ID" ]; then
    echo "新包已是 Developer ID 签名（${{INCOMING_ID}}）→ 保留原签名与公证"
else
    DEVID=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
    if [ -n "$DEVID" ]; then
        codesign --force --sign "$DEVID" --identifier com.kaisersong.beek "$APP"
    else
        echo "⚠️ 没找到 Apple Development 证书，退回 ad-hoc（辅助功能需重新勾选）"
        codesign --force --sign - --identifier com.kaisersong.beek "$APP"
    fi
fi
if [ "{relaunch}" = "1" ]; then
    launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/$LABEL.plist"
else
    echo "（静默安装：不重新拉起，下次启动即为新版本）"
fi
echo "== 替换完成，当前版本 $(/usr/libexec/PlistBuddy -c Print:CFBundleShortVersionString "$APP/Contents/Info.plist" 2>/dev/null) =="
"#,
        version = version,
        new_app = new_app.display(),
        label = LAUNCH_LABEL,
        relaunch = if relaunch { "1" } else { "0" }
    );
    std::fs::write(&script, body).map_err(|e| format!("写替换脚本失败：{}", e))?;
    Ok(script)
}

/// 后台启动替换脚本（调用方随后应当立刻退出自己）
pub fn run_script(script: &Path) -> Result<(), String> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "nohup sh {} >> /tmp/beek_update.log 2>&1 &",
            script.display()
        ))
        .status()
        .map_err(|e| format!("启动替换脚本失败：{}", e))
        .map(|_| ())
}

/// 用系统默认程序打开链接（README / Release 页 / 授权面板都用它）
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("open").arg(url).status();
}

/// 运行中的 bundle 版本（关于窗口用）：CFBundleShortVersionString / CFBundleVersion。
/// 开发态裸跑二进制时读不到，回落到编译期版本。
pub fn bundle_version() -> (String, String) {
    unsafe {
        use cocoa::base::{id, nil};
        use cocoa::foundation::NSString;
        use objc::{class, msg_send, sel, sel_impl};
        let main: id = msg_send![class!(NSBundle), mainBundle];
        let info: id = msg_send![main, infoDictionary];
        let read = |key: &str| -> Option<String> {
            let k: id = NSString::alloc(nil).init_str(key);
            let v: id = msg_send![info, objectForKey: k];
            if v == nil {
                return None;
            }
            let ptr: *const std::os::raw::c_char = msg_send![v, UTF8String];
            if ptr.is_null() {
                None
            } else {
                Some(
                    std::ffi::CStr::from_ptr(ptr)
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        };
        (
            read("CFBundleShortVersionString").unwrap_or_else(|| CURRENT_VERSION.to_string()),
            read("CFBundleVersion").unwrap_or_else(|| "-".to_string()),
        )
    }
}

/// 自检：更新流程里不需要联网的那半段 ——
/// 从解压目录里认出 .app、生成替换脚本。
/// 联网那半段（查 release / 下载）靠真实发布才能端到端验证，见 scripts/release.sh。
pub fn selftest_install() {
    let base = update_dir().join("selftest");
    let _ = std::fs::remove_dir_all(&base);
    let fake = base.join("unpacked").join("BeeK.app");
    if std::fs::create_dir_all(&fake).is_err() {
        eprintln!("[自检] 更新流程：临时目录创建失败，跳过");
        return;
    }
    let found = find_app(&base.join("unpacked"));
    eprintln!(
        "[自检] 解压目录识别 .app：{:?}（期望 .../unpacked/BeeK.app）",
        found.as_ref().map(|p| p.ends_with("BeeK.app"))
    );
    // 再套一层目录也应该能找到（zip 里常见的是「外层目录 + BeeK.app」）
    let nested = base.join("outer").join("inner").join("BeeK.app");
    let _ = std::fs::create_dir_all(&nested);
    eprintln!(
        "[自检] 嵌套两层时仍能找到 .app：{:?}（期望 true）",
        find_app(&base.join("outer")).map(|p| p.ends_with("BeeK.app"))
    );
    match install_script(&fake, "9.9.9") {
        Ok(path) => {
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            // 路径带引号是硬要求：解压目录在 ~/Library/Application Support 下，含空格
            let quoted = body.contains("NEW=\"") && body.contains("APP=\"");
            eprintln!(
                "[自检] 替换脚本已生成：{}（含版本 9.9.9={}，bootout={}，bootstrap={}，路径已加引号={}）",
                path.display(),
                body.contains("9.9.9"),
                body.contains("launchctl bootout"),
                body.contains("launchctl bootstrap"),
                quoted
            );
            if !quoted {
                eprintln!("[自检] ⚠️ 替换脚本路径没加引号：含空格的路径会导致替换失败");
            }
        }
        Err(e) => eprintln!("[自检] 替换脚本生成失败：{}", e),
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// 自检：用本地 HTTP 服务模拟一次真实的「下载 + 解压」。
/// 只做到解压出 .app 为止，不碰 /Applications、不执行替换脚本。
/// 由 BEEK_UPDATE_SELFTEST_URL 触发（默认自检不跑，避免对外发请求）。
pub fn selftest_download(url: &str) {
    let rel = Release {
        version: "9.9.9".to_string(),
        tag: "v9.9.9".to_string(),
        notes: String::new(),
        page_url: RELEASES_URL.to_string(),
        asset_url: Some(url.to_string()),
    };
    eprintln!("[自检] 模拟升级包下载：{}", url);
    match prepare(&rel) {
        Ok(app) => {
            let info = app.join("Contents/Info.plist");
            let ver = std::process::Command::new("/usr/libexec/PlistBuddy")
                .args(["-c", "Print:CFBundleShortVersionString"])
                .arg(&info)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            eprintln!(
                "[自检] 下载+解压成功：{}（目录存在={}，解出来的包版本={}）",
                app.display(),
                app.is_dir(),
                if ver.is_empty() { "读不到" } else { &ver }
            );
        }
        Err(e) => eprintln!("[自检] 下载+解压失败：{}", e),
    }
    // 自检不留垃圾：升级包与解压目录都清掉
    let _ = std::fs::remove_dir_all(update_dir().join("unpacked"));
    let _ = std::fs::remove_file(update_dir().join(asset_name("9.9.9")));
    eprintln!("[自检] 下载+解压自检的临时文件已清理");
}

/// 自检：只跑一遍「检查更新」并打印结果（BEEK_UPDATE_CHECK=1）。
/// 发布 Release 之后用它验证这条链路，不需要点菜单。
pub fn check_selftest() {
    eprintln!("[自检] 当前版本 {}，仓库 {}", CURRENT_VERSION, REPO);
    match check() {
        Ok(None) => eprintln!("[自检] 远端还没有任何 release（链路本身是通的）"),
        Ok(Some(rel)) => {
            eprintln!(
                "[自检] 远端最新 {}（页面 {}，升级包 {:?}）",
                rel.tag, rel.page_url, rel.asset_url
            );
            eprintln!("[自检] 版本说明前 3 行：");
            for line in rel.notes.lines().take(3) {
                eprintln!("[自检]   {}", line);
            }
            if is_newer(&rel.version, CURRENT_VERSION) {
                eprintln!(
                    "[自检] 判定：有新版本（{} → {}）",
                    CURRENT_VERSION, rel.version
                );
            } else {
                eprintln!(
                    "[自检] 判定：已是最新（远端 {} 不比当前 {} 新）",
                    rel.version, CURRENT_VERSION
                );
            }
        }
        Err(e) => eprintln!("[自检] 检查更新失败：{}", e),
    }
    if let Ok(url) = std::env::var("BEEK_UPDATE_PREPARE") {
        eprintln!("[自检] 尝试下载并解压：{}", url);
        let rel = Release {
            version: "selftest".to_string(),
            tag: "selftest".to_string(),
            notes: String::new(),
            page_url: RELEASES_URL.to_string(),
            asset_url: Some(url),
        };
        match prepare(&rel) {
            Ok(app) => eprintln!("[自检] 下载+解压成功：{}", app.display()),
            Err(e) => eprintln!("[自检] 下载+解压失败：{}", e),
        }
    }
}

/// 纯函数自检：版本号比较与资产命名
pub fn selftest() {
    let cases: &[(&str, &str, bool)] = &[
        ("v1.0.1", "1.0.0", true),
        ("1.0.1", "1.0.0", true),
        ("v1.1.0", "1.0.9", true),
        ("v2.0.0", "1.9.9", true),
        ("v1.0.0", "1.0.0", false),
        ("v0.9.9", "1.0.0", false),
        ("v1.0", "1.0.0", false),          // 缺位按 0 补 → 相等
        ("v1.0.2-rc1", "1.0.1", true),     // 预发布后缀忽略
        ("乱七八糟", "1.0.0", false),        // 解析失败一律按「不新」
    ];
    let mut ok = true;
    for (remote, current, want) in cases {
        let got = is_newer(remote, current);
        if got != *want {
            ok = false;
            eprintln!(
                "[自检] 版本比较失败：{} vs {} → {}（期望 {}）",
                remote, current, got, want
            );
        }
    }
    eprintln!(
        "[自检] 版本比较 {} 例：{}；当前版本 {}；资产名 {}；README 锚点 {}",
        cases.len(),
        if ok { "全部通过" } else { "有失败" },
        CURRENT_VERSION,
        asset_name("1.2.3"),
        README_KEY_URL
    );
}
