# R6 保存过的Key状态修复

用户指出Windows显示public-model，且保存后的Key看起来仍为空。public-model与loopback端点是此前实测残留配置，代码默认原本与macOS相同。本次仅对已确认的那两个测试值进行原子配置修复，保留全部其他设置及credential_ref；修复日志确认引用不变。没有在产品中添加检测/重置用户配置的迁移逻辑。

源码改动：TextInput增加saved_secret显示标志，底层content仍为空；仅在masked && saved_secret && content.empty显示固定8星，不加载实际Key。用户输入新Key时显示输入的mask，普通Save仍由value().empty -> KeyEdit::Keep。Form标签明确“已保存”；成功提交无论其他字段是否有未保存编辑，都同步实际提交的credential_ref及显示标记，不改用户输入revision；已有signature守卫继续决定是否清空刚提交的编辑缓冲。Clear成功后引用None，标记关闭；失败不改变标志。

请对本增量找真实可触发问题，尤其是星号误当Key、异步保存新编辑丢失、索引/IME/光标。此前冻结v1源码共识保持历史，本次需新快照；完整G0-G5仍未通过。仅基于附代码复审，给有限PASS或REVISE，不运行工具、不改文件。

```rust
pub struct TextInput {
    masked: bool,
    saved_secret: bool,
    scroll_x: Pixels,
    revision: u64,
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}


```

```rust
        let content = if input.masked && input.saved_secret && input.content.is_empty() {
            // A display-only marker. The actual edit buffer stays empty so a
            // normal settings save keeps the stored credential unchanged.
            "********".into()
        } else if input.masked {
            "*".repeat(input.content.len()).into()
        } else {
            input.content.clone()
        };
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 0., 0.2))
        } else {
            (content, style.color)
        };


```

```rust
    pub fn value(&self) -> String {
        self.content.to_string()
    }
    pub fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        self.content = value.to_owned().into();
        self.selected_range = value.len()..value.len();
        self.marked_range = None;
        cx.notify();
    }
    pub fn set_saved_secret(&mut self, saved: bool, cx: &mut Context<Self>) {
        self.saved_secret = saved;
        cx.notify();
    }
    pub fn mask(&mut self) {
        self.masked = true;
    }

```

```rust
    pub fn new(cx: &mut Context<Self>, placeholder: &str) -> Self {
        Self {
            masked: false,
            saved_secret: false,
            scroll_x: px(0.),
            revision: 0,
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_owned().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

```

```rust
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let mut cfg = self.cfg.clone();
        cfg.provider.model = self.fields[1].read(cx).value().trim().into();
        cfg.provider.base_url = self.fields[2].read(cx).value().trim().into();
        cfg.hotkey = self.fields[3].read(cx).value().trim().into();
        if cfg.hotkey_enabled && native::parse_hotkey(&cfg.hotkey).is_none() {
            self.status = "快捷键格式例如 Ctrl+Alt+T；需修饰键和一个英文字母".into();
            cx.notify();
            return;
        }
        if let Err(e) = cfg.validate() {
            self.status = e;
            cx.notify();
            return;
        }
        let key = self.fields[0].read(cx).value();
        let edit = if self.clear {
            KeyEdit::Clear
        } else if key.trim().is_empty() {
            KeyEdit::Keep
        } else {
            KeyEdit::Replace(key)
        };
        if self.tx.try_send(Command::Save(cfg, edit)).is_ok() {
            self.busy = true;
            self.save_revision = Some(self.signature(cx));
            self.status = "正在保存…".into();
            cx.notify();
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            if self.cfg.provider.credential_ref.is_some() {
                "API Key（已保存；输入新 Key 可替换）"
            } else {
                "API Key（输入后显示为星号）"
            },
            "模型 / 推理接入点",
            "完整 API 地址（chat/completions）",
            "全局快捷键（默认 Ctrl+Alt+T）",
        ];
        let mut view = div()
            .id("settings")
            .size_full()
            .p_6()
            .bg(rgb(0xf8f7f3))
            .text_color(rgb(0x252525))
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .key_context("BeeKSettings")
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::save))
            .child(div().text_xl().child("BeeK 设置"));
        for (i, label) in labels.into_iter().enumerate() {
            view = view
                .child(div().text_sm().child(label))
                .child(self.fields[i].clone());
        }
        let flags = [
            ("选中文本后显示蜜蜂按钮", self.cfg.selection_enabled),
            ("复制文本后显示（默认关闭）", self.cfg.copy_to_popup),
            ("启用全局快捷键", self.cfg.hotkey_enabled),
            ("登录 Windows 时启动", self.cfg.autostart),
            ("清除已保存的 API Key", self.clear),
        ];
        for (i, (label, on)) in flags.into_iter().enumerate() {
            view = view.child(
                div()
                    .id(i)
                    .cursor_pointer()
                    .text_sm()
                    .child(format!("{} {label}", if on { "☑" } else { "☐" }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, _, cx| {
                            match i {
                                0 => s.cfg.selection_enabled = !s.cfg.selection_enabled,
                                1 => s.cfg.copy_to_popup = !s.cfg.copy_to_popup,
                                2 => s.cfg.hotkey_enabled = !s.cfg.hotkey_enabled,
                                3 => s.cfg.autostart = !s.cfg.autostart,
                                _ => s.clear = !s.clear,
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        view.child(
            div()
                .text_sm()
                .text_color(rgb(0x795a22))
                .child(self.status.clone()),
        )
        .when(!native::HOTKEY_OK.load(Ordering::Acquire), |d| {
            d.child("快捷键注册失败，可能被其他应用占用；请修改后保存。")
        })
        .child(
            div()
                .id("save")
                .p_2()
                .rounded_md()
                .bg(rgb(0xffd359))
                .cursor_pointer()
                .child(if self.busy {
                    "保存中…"
                } else {
                    "保存 (Ctrl+S)"
                })
                .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, cx| s.submit(cx))),
        )
        .child(
            div()
                .id("updates")
                .text_sm()
                .cursor_pointer()
                .child(format!("BeeK {} · 手动检查更新", env!("CARGO_PKG_VERSION")))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::Update);
                    }),
                ),
        )
        .child(
            div()
                .text_xs()
                .child("仅点击翻译后发送所选文本。剪贴板回退不会模拟 Ctrl+C；不自动下载安装更新。"),
        )
    }
}
fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> Option<WindowHandle<Form>> {
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("BeeK Settings".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(720.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                let mut fields = Vec::new();
                for (i, (label, value)) in [
                    ("API Key", ""),
                    ("model", cfg.provider.model.as_str()),
                    ("API URL", cfg.provider.base_url.as_str()),
                    ("hotkey", cfg.hotkey.as_str()),
                ]
                .into_iter()
                .enumerate()
                {
                    let field = cx.new(|cx| {
                        let mut f = TextInput::new(cx, label);
                        f.set_value(value, cx);
                        if i == 0 {
                            f.mask();
                            f.set_saved_secret(cfg.provider.credential_ref.is_some(), cx);
                        }
                        f
                    });
                    fields.push(field);
                }
                fields[0].read(cx).focus(w);
                cx.new(|cx| {
                    let subscriptions = fields
                        .iter()
                        .map(|field| {
                            cx.subscribe(
                                field,
                                |this: &mut Form, field, event: &ClipboardIntent, cx| {
                                    if this
                                        .tx
                                        .try_send(Command::Edit(field, event.clone()))
                                        .is_err()
                                    {
                                        this.status = "操作队列繁忙，请稍后重试".into();
                                        cx.notify();
                                    }
                                },
                            )
                        })
                        .collect();
                    Form {
                        _subscriptions: subscriptions,
                        save_revision: None,
                        fields,
                        cfg,
                        status,
                        busy: false,
                        clear: false,
                        tx,
                    }
                })
            },
        )
        .map_err(|_| {
            super::diagnostics::event("window_open_failed", serde_json::json!({"kind":"settings"}));
            util::fatal("无法创建设置窗口，请检查显卡驱动后重试");
        })
        .ok()?;
    let _ = handle.update(cx, |_, w, _| {
        let _ = native::keep_accessible(w);
        native::activate(w)
    });
    Some(handle)
}

```

保存回执：
```rust
     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status;if committed{if unchanged{f.cfg=cfg.clone();}else{f.status.push_str("；提交后的新修改尚未保存");}f.cfg.provider.credential_ref=cfg.provider.credential_ref.clone();f.fields[0].update(cx,|key,cx|key.set_saved_secret(cfg.provider.credential_ref.is_some(),cx));if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}cx.notify();});});},
```

## src/core/settings.rs
```rust
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
        if self.provider.base_url.len() > 4096
            || self.hotkey.len() > 64
            || self
                .provider
                .credential_ref
                .as_ref()
                .is_some_and(|r| r.is_empty() || r.len() > 128)
            || self.provider.api_key.len() > 2560
        {
            return Err("配置字段超出长度限制，原配置已保留".into());
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
    let edit = match edit {
        KeyEdit::Keep
            if old.provider.credential_ref.is_none() && !old.provider.api_key.is_empty() =>
        {
            KeyEdit::Replace(old.provider.api_key.clone())
        }
        edit => edit,
    };
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
    #[test]
    fn keeping_a_legacy_key_migrates_transactionally_instead_of_erasing_it() {
        let mut old = Settings::default();
        old.provider.api_key = "public-legacy-fixture".into();
        let mut memory = Memory {
            fail: true,
            ..Default::default()
        };
        assert!(transact(&mut memory, &old, old.clone(), KeyEdit::Keep).is_err());
        assert_eq!(old.provider.api_key, "public-legacy-fixture");
        assert!(memory.secrets.is_empty());
        memory.fail = false;
        let saved = transact(&mut memory, &old, old.clone(), KeyEdit::Keep).unwrap();
        assert_eq!(
            memory
                .read_secret(saved.provider.credential_ref.as_ref().unwrap())
                .unwrap(),
            "public-legacy-fixture"
        );
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("public-legacy-fixture"));
    }
    #[test]
    fn accepted_configuration_fields_are_bounded_for_reload() {
        let mut s = Settings::default();
        s.provider.base_url = format!("https://example.invalid/{}", "x".repeat(4096));
        assert!(s.validate().is_err());
        s.provider.base_url = Provider::default().base_url;
        s.hotkey = "x".repeat(65);
        assert!(s.validate().is_err());
    }
}

```
