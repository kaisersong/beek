// BeeK 原生对话框 — 设置 API Key / 关于 / 更新提示
//
// 为什么用 AppKit 的 NSAlert 而不是 GPUI 窗口：gpui 0.2.2 没有文本输入控件
// （TextInput 在 Zed 的 ui crate 里，本项目没引这个依赖），而配置 Key 必须有输入框。
// NSAlert + NSTextField 是系统标准做法，样式也跟菜单栏 app 的定位一致。
//
// 线程约束：全部只能在主线程调用（菜单动作和 GPUI 主循环都在主线程，
// 后台线程只能把结果通过 UiEvent 送回主循环再弹）。
use cocoa::base::{id, nil};
use beek::core::localization;
use cocoa::foundation::{NSInteger, NSPoint, NSRect, NSSize, NSString};
use objc::runtime::{NO, YES};
use objc::{class, msg_send, sel, sel_impl};

fn uistr(s:&str)->id {nsstr(localization::tr(s))}

fn nsstr(s: &str) -> id {
    unsafe {
        let value = NSString::alloc(nil).init_str(s);
        let _: id = msg_send![value, autorelease];
        value
    }
}

/// 自检模式：不真的弹窗、不阻塞，直接返回「第一个按钮」。
/// 这样 BEEK_SELFTEST 能把「菜单 → 主循环 → 对话框 → 落盘」这条链路跑通，
/// 而不需要有人坐在屏幕前点按钮。生产（没设这个环境变量）时行为完全不变。
fn selftest_mode() -> bool {
    std::env::var("BEEK_SELFTEST").is_ok()
}

unsafe fn nsstring_value(v: id) -> String {
    if v == nil {
        return String::new();
    }
    let ptr: *const std::os::raw::c_char = msg_send![v, UTF8String];
    if ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// 弹一个原生提示框，返回被点按钮的下标（0 = 第一个按钮）。
/// 弹模态框前把 app 抬成「前台 + 有菜单栏」的状态，返回原来的激活策略（需要恢复时用）。
///
/// 为什么要做这件事（两个现象同一个根因）：
/// 模态框要求 app 处于激活状态。不激活就弹，AppKit 会去「请求用户注意」——
/// 有 Dock 图标时表现就是 **Dock 图标一直跳**；而菜单里关掉「在 Dock 中显示」之后，
/// app 是 Accessory，Accessory 按定义**没有菜单栏**，被激活后菜单栏会显示成灰的。
/// 所以这里临时切到 Regular（有 Dock 图标 + 正常菜单栏），框关掉再切回原策略。
unsafe fn enter_dialog_context() -> Option<i64> {
    let app: id = msg_send![class!(NSApplication), sharedApplication];
    let policy: i64 = msg_send![app, activationPolicy];
    if policy != 0 {
        let _: () = msg_send![app, setActivationPolicy: 0i64]; // 0 = Regular
        eprintln!("[beek] 弹对话框：临时切到 Regular（原策略 {}，框关掉后恢复）", policy);
    }
    // activateIgnoringOtherApps: 在 14.0 标记为 deprecated，但各版本都还能用；
    // 换成 activate() 的话老系统没有这个选择器。
    let _: () = msg_send![app, activateIgnoringOtherApps: true];
    // 让 AX 轮询别再问 AX：现在前台是我们自己，问自己会死锁（见 ax::set_self_ui_front）
    crate::ax::set_self_ui_front(true);
    if policy != 0 {
        Some(policy)
    } else {
        None
    }
}

/// 对话框结束，恢复原来的激活策略（Accessory = 没有 Dock 图标）
unsafe fn leave_dialog_context(prev: Option<i64>) {
    crate::ax::set_self_ui_front(false);
    let app: id = msg_send![class!(NSApplication), sharedApplication];
    // isActive 是「不再请求用户注意（Dock 图标不跳）」的直接证据：
    // 模态框期间 app 真的在前台，就不需要 AppKit 去请求注意了
    let active: i8 = msg_send![app, isActive];
    match prev {
        Some(p) => {
            let _: () = msg_send![app, setActivationPolicy: p];
            eprintln!(
                "[beek] 对话框期间 isActive={}；激活策略恢复为 {}（=去掉临时 Dock 图标）",
                active != 0,
                p
            );
        }
        None => eprintln!("[beek] 对话框期间 isActive={}（本来就是 Regular）", active != 0),
    }
}

/// NSAlert 的返回值从 1000 起算（NSAlertFirstButtonReturn）。
pub fn alert(title: &str, body: &str, buttons: &[&str]) -> usize {
    if selftest_mode() {
        eprintln!(
            "[自检] 对话框「{}」：{}｜按钮 {:?} → 自检模式返回第 1 个",
            title,
            body.replace('\n', " / "),
            buttons
        );
        return 0;
    }
    unsafe {
        let alert: id = msg_send![class!(NSAlert), new];
        let _: () = msg_send![alert, setMessageText: uistr(title)];
        if !body.is_empty() {
            let _: () = msg_send![alert, setInformativeText: uistr(body)];
        }
        let _: () = msg_send![alert, setAlertStyle: 1i64]; // NSAlertStyleInformational
        for b in buttons {
            let _: id = msg_send![alert, addButtonWithTitle: uistr(b)];
        }
        let prev = enter_dialog_context();
        // 诊断：BEEK_DIALOG_AUTOCLOSE=1 时 2 秒后自动「按好」，
        // 用来验证「框关掉之后」的状态（那时 app 可能还在最前面）
        if std::env::var("BEEK_DIALOG_AUTOCLOSE").is_ok() {
            crate::menubar::stop_modal_async(std::time::Duration::from_secs(2));
        }
        let resp: NSInteger = msg_send![alert, runModal];
        let _: () = msg_send![alert,release];
        leave_dialog_context(prev);
        (resp - 1000).max(0) as usize
    }
}

/// 「设置 API Key…」的输入结果
pub struct KeyInput {
    pub api_key: String,
    pub model: String,
}

pub fn cache_settings_dialog(settings: beek::core::translation_cache::CacheSettings)
    -> Option<beek::core::translation_cache::CacheSettings> {
    use beek::core::translation_cache::{CacheSettings, MAX_ENTRIES, MAX_TTL_MINUTES};
    let mut entries = settings.max_entries.to_string();
    let mut minutes = settings.ttl_minutes.to_string();
    let mut error = String::new();
    loop {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        let (response, next_entries, next_minutes) = unsafe {
            let prev = enter_dialog_context();
            let view: id = msg_send![class!(NSView), alloc];
            let view: id = msg_send![view, initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 96.))];
            let entries_label = label(&format!("{} (0–{}, {})",localization::tr("缓存条数"),MAX_ENTRIES,localization::tr("0 表示关闭")), NSRect::new(NSPoint::new(0., 72.), NSSize::new(420., 16.)));
            let entries_field = field(&entries, "128", NSRect::new(NSPoint::new(0., 46.), NSSize::new(420., 24.)));
            let minutes_label = label(&format!("{} (0–{}, {})",localization::tr("有效时间（分钟）"),MAX_TTL_MINUTES,localization::tr("0 表示关闭")), NSRect::new(NSPoint::new(0., 26.), NSSize::new(420., 16.)));
            let minutes_field = field(&minutes, "30", NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 24.)));
            let _: () = msg_send![entries_field, setIdentifier: nsstr("beek-cache-max-entries")];
            let _: () = msg_send![minutes_field, setIdentifier: nsstr("beek-cache-ttl-minutes")];
            for child in [entries_label, entries_field, minutes_label, minutes_field] {
                let _: () = msg_send![view, addSubview: child];
            }
            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText: uistr("翻译缓存设置")];
            let body = if error.is_empty() {
                "相同内容优先复用已完成的翻译。\n所有翻译服务统一使用结果缓存（包括百度）。\n缓存仅保留在内存，最多 2MiB，退出后清空。\n保存后立即生效。".to_owned()
            } else { error.clone() };
            let _: () = msg_send![alert, setInformativeText: uistr(&body)];
            let _: () = msg_send![alert, setAccessoryView: view];
            let _: id = msg_send![alert, addButtonWithTitle: uistr("保存")];
            let _: id = msg_send![alert, addButtonWithTitle: uistr("取消")];
            let window: id = msg_send![alert, window];
            let _: () = msg_send![window, makeFirstResponder: entries_field];
            let response: NSInteger = msg_send![alert, runModal];
            let next_entries = nsstring_value(msg_send![entries_field, stringValue]);
            let next_minutes = nsstring_value(msg_send![minutes_field, stringValue]);
            let _: () = msg_send![alert, release];
            let _: () = msg_send![view, release];
            for child in [entries_label, entries_field, minutes_label, minutes_field] {
                let _: () = msg_send![child, release];
            }
            leave_dialog_context(prev);
            (response, next_entries, next_minutes)
        };
        if response != 1000 { return None; }
        entries = next_entries;
        minutes = next_minutes;
        match CacheSettings::from_input(&entries, &minutes) {
            Ok(settings) => return Some(settings),
            Err(message) => error = if localization::is_english(){format!("Enter whole numbers: entries 0–{MAX_ENTRIES}, lifetime 0–{MAX_TTL_MINUTES} minutes. Zero disables the cache.")}else{message},
        }
    }
}

#[cfg(feature = "test-fixtures")]
thread_local! {
    static STORAGE_UI_CHECK: std::cell::RefCell<Option<(crate::config::CredentialStorage, crate::config::CredentialStorage, bool)>> = const { std::cell::RefCell::new(None) };
}

/// Exercise actual modal controls on the main thread; never touches user settings.
#[cfg(feature = "test-fixtures")]
pub fn storage_choice_selftest(config: &crate::config::Config, next: crate::config::CredentialStorage, cancel: bool)
    -> Option<(crate::config::Engine, crate::config::ProviderConfig, crate::config::CredentialStorage)> {
    STORAGE_UI_CHECK.with(|plan| *plan.borrow_mut()=Some((config.credential_storage(crate::config::ARK),next,cancel)));
    let result=doubao_settings_dialog(config);
    assert!(STORAGE_UI_CHECK.with(|plan|plan.borrow().is_none()),"native storage test did not reach its controls");
    result
}

/// Native picker then provider-specific fields. Nothing is persisted until Save.
pub fn translation_settings_dialog(config: &crate::config::Config)
    -> Option<(crate::config::Engine, crate::config::ProviderConfig, crate::config::CredentialStorage)> {
    translation_settings_starting_at(config,None)
}

pub fn doubao_settings_dialog(config: &crate::config::Config) -> Option<(crate::config::Engine,crate::config::ProviderConfig,crate::config::CredentialStorage)> {
    translation_settings_starting_at(config,Some(crate::config::Engine::Ark))
}

fn translation_settings_starting_at(config:&crate::config::Config,mut direct:Option<crate::config::Engine>) -> Option<(crate::config::Engine,crate::config::ProviderConfig,crate::config::CredentialStorage)> {
    use crate::config::Engine;
    let mut selected = config.translation_engine;
    let mut drafts = config.providers.clone();
    let mut hydrated = std::collections::BTreeSet::new();
    let mut storage_drafts = config.credential_storage.clone();
    loop {
        let picked = if let Some(engine)=direct.take() { Some(engine) } else { unsafe {
            let _pool = crate::macos_runtime::AutoreleasePool::new();
            let prev = enter_dialog_context();
            let view: id = msg_send![class!(NSView), alloc];
            let view: id = msg_send![view, initWithFrame: NSRect::new(NSPoint::new(0.,0.),NSSize::new(460.,44.))];
            let picker: id = msg_send![class!(NSPopUpButton), alloc];
            let picker: id = msg_send![picker, initWithFrame: NSRect::new(NSPoint::new(0.,8.),NSSize::new(460.,28.)) pullsDown: NO];
            for engine in Engine::ALL { let _: () = msg_send![picker, addItemWithTitle: uistr(engine.label())]; }
            let index = Engine::ALL.iter().position(|e|*e==selected).unwrap_or(Engine::ALL.len());
            if index == Engine::ALL.len() { let _: () = msg_send![picker, addItemWithTitle: uistr(Engine::Unsupported.label())]; }
            let _: () = msg_send![picker, selectItemAtIndex: index as NSInteger];
            let _: () = msg_send![picker, setIdentifier: nsstr("beek-translation-engine")];
            let _: () = msg_send![view, addSubview: picker];
            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText: uistr("翻译服务设置")];
            let _: () = msg_send![alert, setInformativeText: uistr("选择翻译引擎，再配置并启用。每个引擎的设置单独保留。\n不会自动切换服务；仅在你点击翻译后发送选区。")];
            let _: () = msg_send![alert, setAccessoryView: view];
            let _: id = msg_send![alert, addButtonWithTitle: uistr("配置并启用…")];
            let _: id = msg_send![alert, addButtonWithTitle: uistr("取消")];
            let response: NSInteger = msg_send![alert, runModal];
            let chosen: NSInteger = msg_send![picker, indexOfSelectedItem];
            let _: () = msg_send![alert, release]; let _: () = msg_send![view, release]; let _: () = msg_send![picker, release];
            leave_dialog_context(prev);
            if response==1000 { Engine::ALL.get(chosen as usize).copied() } else { None }
        }}?;
        selected = picked;
        if hydrated.insert(selected.key()) {
            match config.try_provider(selected.key()) {
                Ok(provider) => { drafts.insert(selected.key().into(), provider); }
                Err(error) => { alert("翻译服务未保存", &error, &["好"]); return None; }
            }
        }
        let mut provider = drafts.get(selected.key()).cloned().unwrap_or_default();
        let mut storage = storage_drafts.get(selected.key()).copied().unwrap_or_else(||config.credential_storage(selected.key()));
        let mut error = String::new();
        loop {
            let (response, values, next_storage) = unsafe {
                let _pool = crate::macos_runtime::AutoreleasePool::new();
                let prev = enter_dialog_context();
                // key, label, placeholder, value, secure
                let specs: Vec<(&str,&str,&str,String,bool)> = match selected {
                    Engine::PublicBaidu => vec![],
                    Engine::Ark => vec![("key","API Key（也可使用 BEEK_ARK_KEY）","API Key",provider.api_key.clone(),true),("model","模型 / 接入点 ID（留空用默认模型）","doubao-seed-2-0-mini-260428",provider.model.clone().unwrap_or_default(),false),("url","OpenAI 兼容服务完整地址（可留空）","https://…/chat/completions",provider.base_url.clone().unwrap_or_default(),false)],
                    Engine::Azure => vec![("key","Azure 资源密钥","资源密钥",provider.api_key.clone(),true),("region","资源区域（全球资源可留空）","例如 eastasia",provider.region.clone(),false)],
                    Engine::Baidu => vec![("app_id","百度翻译 App ID","App ID",provider.app_id.clone(),false),("key","百度翻译密钥","密钥",provider.api_key.clone(),true)],
                    Engine::Google => vec![("key","Google Cloud Translation API Key","API Key",provider.api_key.clone(),true)],
                    Engine::MyMemory => vec![("source","源语言代码（不支持自动识别）","en / ja / fr",if provider.source_language.is_empty(){"en".into()}else{provider.source_language.clone()},false),("email","有效邮箱（可留空）","可选邮箱",provider.email.clone(),false)],
                    Engine::LibreTranslate => vec![("url","LibreTranslate / Argos 服务地址","http://127.0.0.1:5000/translate",provider.base_url.clone().unwrap_or_default(),false),("key","服务 API Key（本地服务可留空）","可选 API Key",provider.api_key.clone(),true)],
                    _ => unreachable!(),
                };
                let has_key = specs.iter().any(|(key,_,_,_,_)| *key == "key");
                let storage_height = if has_key { 64. } else { 0. };
                let height = (specs.len() * 58) as f64 + storage_height;
                let view: id = msg_send![class!(NSView), alloc];
                let view: id = msg_send![view, initWithFrame: NSRect::new(NSPoint::new(0.,0.),NSSize::new(460.,height))];
                let mut owned = Vec::new(); let mut fields = Vec::new();
                for (index,(key,title,placeholder,value,secure)) in specs.iter().enumerate() {
                    let y = height - (index as f64 + 1.) * 58.;
                    let caption = label(title, NSRect::new(NSPoint::new(0.,y+30.),NSSize::new(460.,18.)));
                    let input: id = if *secure {
                        let alloc: id = msg_send![class!(NSSecureTextField), alloc];
                        let field: id = msg_send![alloc, initWithFrame: NSRect::new(NSPoint::new(0.,y+2.),NSSize::new(460.,26.))];
                        let _: () = msg_send![field, setStringValue: nsstr(value)]; let _: () = msg_send![field, setEditable: YES]; field
                    } else { field(value,placeholder,NSRect::new(NSPoint::new(0.,y+2.),NSSize::new(460.,26.))) };
                    let _: () = msg_send![input, setIdentifier: nsstr(&format!("beek-engine-{key}"))];
                    let _: () = msg_send![view, addSubview: caption]; let _: () = msg_send![view, addSubview: input];
                    owned.extend([caption,input]); fields.push((key.to_string(),input));
                }
                let storage_picker = if has_key {
                    let caption = label("API Key 保存位置", NSRect::new(NSPoint::new(0.,34.),NSSize::new(460.,18.)));
                    let picker: id = msg_send![class!(NSPopUpButton), alloc];
                    let picker: id = msg_send![picker, initWithFrame: NSRect::new(NSPoint::new(0.,2.),NSSize::new(460.,28.)) pullsDown: NO];
                    for title in ["配置文件（明文）", "macOS 钥匙串"] { let _: () = msg_send![picker, addItemWithTitle: uistr(title)]; }
                    let index: NSInteger = if storage == crate::config::CredentialStorage::Keychain { 1 } else { 0 };
                    let _: () = msg_send![picker, selectItemAtIndex: index];
                    let _: () = msg_send![picker, setIdentifier: nsstr("beek-credential-storage")];
                    let _: () = msg_send![view, addSubview: caption];
                    let _: () = msg_send![view, addSubview: picker];
                    owned.extend([caption,picker]);
                    Some(picker)
                } else { None };
                let alert: id = msg_send![class!(NSAlert), new];
                let _: () = msg_send![alert, setMessageText: uistr(selected.label())];
                let help = if !error.is_empty(){error.clone()}else{match selected {
                    Engine::Ark=>"提供流式翻译与单词详解。凭据只保存在本机。",
                    Engine::PublicBaidu=>"无需 Key，初次使用默认启用。点击翻译后，选中文本经 Cloudflare 转发给百度；公共服务端不保存原文/译文；本机可按缓存设置复用译文。共享免费额度有限，单次最多 1500 字节；繁忙或额度用尽时可切换自己的百度 Key 或其他服务。",
                    Engine::Baidu=>"翻译为菜单中选择的语言。请填写自己的 App ID 和密钥。支持结果缓存。",
                    Engine::MyMemory=>"翻译为菜单中选择的语言；单次最多 500 字节。需要明确源语言。邮箱为可选项。",
                    Engine::LibreTranslate=>"翻译为菜单中选择的语言。需要已运行的 LibreTranslate 服务（可使用 Argos 模型）；BeeK 不会自动安装或启动它。",
                    _=>"翻译为菜单中选择的语言。请先开通对应翻译 API；保存配置本身不会发起网络请求。",
                }.into()};
                let _: () = msg_send![alert, setInformativeText: uistr(&help)];
                let _: () = msg_send![alert, setAccessoryView: view];
                for title in ["保存并启用","返回","取消"] { let _: id = msg_send![alert, addButtonWithTitle: uistr(title)]; }
                if selected == Engine::LibreTranslate { let _: id = msg_send![alert, addButtonWithTitle: uistr("本地安装说明…")]; }
                if let Some((_,first))=fields.first() { let window: id = msg_send![alert, window]; let _: () = msg_send![window, makeFirstResponder: *first]; }
                #[cfg(feature = "test-fixtures")]
                if let Some((expected,next,cancel))=STORAGE_UI_CHECK.with(|plan|plan.borrow_mut().take()) {
                    let picker=storage_picker.expect("API Key storage picker missing");
                    let index: NSInteger=msg_send![picker,indexOfSelectedItem];
                    assert_eq!(index,if expected==crate::config::CredentialStorage::Keychain {1}else{0},"incorrect persisted storage choice");
                    let count: NSInteger=msg_send![picker,numberOfItems];
                    assert_eq!(count,2);
                    let index: NSInteger=if next==crate::config::CredentialStorage::Keychain {1}else{0};
                    let _: ()=msg_send![picker,selectItemAtIndex:index];
                    let buttons: id=msg_send![alert,buttons];
                    let button: id=msg_send![buttons,objectAtIndex:if cancel {2usize}else{0usize}];
                    let modes: id=msg_send![class!(NSArray),arrayWithObject:nsstr("NSModalPanelRunLoopMode")];
                    let _: ()=msg_send![button,performSelector:sel!(performClick:) withObject:nil afterDelay:0.2f64 inModes:modes];
                }
                let response: NSInteger = msg_send![alert, runModal];
                let next_storage = storage_picker.map(|picker| {
                    let index: NSInteger = msg_send![picker, indexOfSelectedItem];
                    if index == 1 { crate::config::CredentialStorage::Keychain } else { crate::config::CredentialStorage::File }
                }).unwrap_or(storage);
                let values=fields.iter().map(|(key,input)|(key.clone(),nsstring_value(msg_send![*input,stringValue]).trim().to_owned())).collect::<Vec<_>>();
                let _: () = msg_send![alert, release]; let _: () = msg_send![view, release];
                for object in owned { let _: () = msg_send![object, release]; }
                leave_dialog_context(prev);
                (response,values,next_storage)
            };
            storage = next_storage;
            for (key,value) in values { match key.as_str() {
                "key"=>provider.api_key=value,"model"=>provider.model=(!value.is_empty()).then_some(value),"url"=>provider.base_url=(!value.is_empty()).then_some(value),"region"=>provider.region=value,"app_id"=>provider.app_id=value,"source"=>provider.source_language=value,"email"=>provider.email=value,_=>{}
            }}
            match response {
                1000=>match crate::config::validate_engine_settings(selected,&provider) { Ok(())=>return Some((selected,provider,storage)),Err(message)=>error=message },
                1001=>{ drafts.insert(selected.key().into(),provider); storage_drafts.insert(selected.key().into(),storage); break; },
                1003 if selected == Engine::LibreTranslate => local_translation_help_dialog(),
                _=>return None,
            }
        }
    }
}

/// Bundled user help: available offline, independent of the external docs repo.
fn local_translation_help_dialog() {
    let guide=localization::choose(include_str!("../resources/help/libretranslate-install.txt"),include_str!("../resources/help/libretranslate-install.en.txt"));
    let prompt=localization::choose(include_str!("../resources/help/libretranslate-agent-prompt.txt"),include_str!("../resources/help/libretranslate-agent-prompt.en.txt"));
    let mut feedback = String::new();
    loop {
        let response = unsafe {
            let _pool = crate::macos_runtime::AutoreleasePool::new();
            let previous = enter_dialog_context();
            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText: uistr("本地翻译安装说明")];
            let body = format!("{}LibreTranslate 提供接口，Argos 提供本机模型。\n\n1. 推荐点击“复制 AI 安装提示词”，粘贴给能操作终端的 AI 助手（如小K），完成检查、安装和验证。\n\n2. 手动安装可选 Docker 或独立 Python 环境；点击“复制完整安装说明”可取得所有命令、启动/停止和排错方法。\n\n3. 首次需要联网下载 en/zh 模型。服务就绪后，用公开短句测试 /translate，确认返回非空 translatedText。\n\n4. 返回配置填写：\nhttp://127.0.0.1:5000/translate\nAPI Key 默认留空，然后保存并启用。\n\n只监听本机；复制不会自动执行安装。速度取决于电脑和文本长度。",feedback);
            let body=if localization::is_english(){format!("{}{}",localization::tr(&feedback),guide)}else{body};
            let _: () = msg_send![alert, setInformativeText: uistr(&body)];
            for title in ["返回配置","复制 AI 安装提示词","复制完整安装说明","官方安装文档"] { let _: id = msg_send![alert, addButtonWithTitle: uistr(title)]; }
            let result: NSInteger = msg_send![alert, runModal];
            let _: () = msg_send![alert, release];
            leave_dialog_context(previous);
            result
        };
        match response {
            1001 => { crate::clipboard::write_string(prompt); crate::clipboard::note_own_write(); feedback = "AI 安装提示词已复制。\n\n".into(); },
            1002 => { crate::clipboard::write_string(guide); crate::clipboard::note_own_write(); feedback = "完整安装说明已复制。\n\n".into(); },
            1003 => crate::update::open_url("https://docs.libretranslate.com/guides/installation/"),
            _ => return,
        }
    }
}

fn label(text: &str, frame: NSRect) -> id {
    unsafe {
        let l: id = msg_send![class!(NSTextField), alloc];
        let l: id = msg_send![l, initWithFrame: frame];
        let _: () = msg_send![l, setStringValue: uistr(text)];
        let _: () = msg_send![l, setBezeled: NO];
        let _: () = msg_send![l, setEditable: NO];
        let _: () = msg_send![l, setSelectable: NO];
        let _: () = msg_send![l, setDrawsBackground: NO];
        l
    }
}

fn field(value: &str, placeholder: &str, frame: NSRect) -> id {
    unsafe {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f, initWithFrame: frame];
        let _: () = msg_send![f, setStringValue: nsstr(value)];
        let _: () = msg_send![f, setPlaceholderString: uistr(placeholder)];
        let _: () = msg_send![f, setEditable: YES];
        f
    }
}

/// 配置 Key 的对话框。返回 None 表示用户取消。
///
/// 三个按钮：保存 / 取消 / 如何获取 Key…（第三个会打开 README 对应章节，
/// 并把已经输入的内容原样带回，重新弹一次，不丢用户填的东西）。
pub fn key_settings_dialog(mut key: String, mut model: String) -> Option<KeyInput> {
    if selftest_mode() {
        // 用一个固定长度的假 Key，只验证「走通保存链路」；自检跑在临时 HOME 下，
        // 不会碰用户真实的 config.json
        eprintln!("[自检] 设置对话框：自检模式返回固定的假 Key（不读输入框）");
        return Some(KeyInput {
            api_key: "selftest-ark-key-0000".to_string(),
            model: "selftest-model".to_string(),
        });
    }
    loop {
        let choice = unsafe {
            // 先激活再建框：不激活的话输入框拿不到键盘（文字光标也点不进去），
            // 而且 AppKit 会去请求用户注意（Dock 图标跳）
            let prev = enter_dialog_context();
            let view: id = msg_send![class!(NSView), alloc];
            let view: id = msg_send![view,
                initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 96.))];

            let key_label = label("豆包 Ark API Key（必填）", NSRect::new(NSPoint::new(0., 72.), NSSize::new(420., 16.)));
            let key_field = field(&key, "粘贴你的 Ark API Key", NSRect::new(NSPoint::new(0., 46.), NSSize::new(420., 24.)));
            let model_label = label("模型 / 推理接入点 ID（可留空，用默认模型）", NSRect::new(NSPoint::new(0., 26.), NSSize::new(420., 16.)));
            let model_field = field(&model, "doubao-seed-2-0-mini-260428", NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 24.)));
            let _: () = msg_send![view, addSubview: key_label];
            let _: () = msg_send![view, addSubview: key_field];
            let _: () = msg_send![view, addSubview: model_label];
            let _: () = msg_send![view, addSubview: model_field];

            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText: uistr("BeeK 翻译服务设置")];
            let _: () = msg_send![alert, setInformativeText: uistr(
                localization::choose("Key 只保存在本机：~/Library/Application Support/BeeK/config.json。\n也可以改用环境变量 BEEK_ARK_KEY（此处的配置优先）。","Your key is stored only on this Mac in ~/Library/Application Support/BeeK/config.json.\nYou can also use BEEK_ARK_KEY; saved settings take priority.")
            )];
            let _: () = msg_send![alert, setAccessoryView: view];
            let _: () = msg_send![alert, addButtonWithTitle: uistr("保存")];
            let _: () = msg_send![alert, addButtonWithTitle: uistr("取消")];
            let _: () = msg_send![alert, addButtonWithTitle: uistr("如何获取 Key…")];

            // 让光标直接落在 Key 输入框里
            let win: id = msg_send![alert, window];
            let _: () = msg_send![win, makeFirstResponder: key_field];

            let resp: NSInteger = msg_send![alert, runModal];
            let idx = (resp - 1000).max(0) as usize;

            let k = nsstring_value(msg_send![key_field, stringValue]);
            let m = nsstring_value(msg_send![model_field, stringValue]);
            leave_dialog_context(prev);
            (idx, k, m)
        };

        let (idx, k, m) = choice;
        match idx {
            0 => {
                return Some(KeyInput {
                    api_key: k.trim().to_string(),
                    model: m.trim().to_string(),
                })
            }
            2 => {
                // 先去看怎么拿 Key，回来接着填：把已输入内容带回
                crate::update::open_url(crate::update::README_KEY_URL);
                key = k;
                model = m;
                continue;
            }
            _ => return None,
        }
    }
}

/// 「关于 BeeK」：版本信息 + 两个跳转入口。返回被点按钮下标
/// （0 = 检查更新…，1 = 打开 README，2 = 好）
pub fn about_dialog() -> usize {
    let (version, build) = crate::update::bundle_version();
    let body = format!(
        "轻量 macOS AI 划词翻译工具\n\n\
         版本 v{}（构建号 {}）\n\
         仓库 github.com/{}\n\n\
         选中文本 → 点小蜜蜂 → 看译文。\n\
         单个词会给词典式详解（读音、词性、例句、搭配），点喇叭可朗读。",
        version,
        build,
        crate::update::REPO
    );
    let body=if localization::is_english(){format!("A lightweight selection translator for macOS\n\nVersion v{} (build {})\nRepository: github.com/{}\n\nSelect text, click the bee, and read its translation.\nSingle words can include pronunciation, definitions, examples, and usage. Click the speaker to listen.",version,build,crate::update::REPO)}else{body};
    // 标题不再带 emoji：NSAlert 的图标本来就是 app 自己的图标（真实素材），
    // 再写一个 🐝 字形等于在真图标下面又贴了个假图标。
    alert("BeeK", &body, &["检查更新…", "打开 README", "好"])
}
