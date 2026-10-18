//! One fixed STA voice worker, at most one queued utterance and bounded results.
use std::sync::mpsc;
pub struct Speech {
    input: mpsc::SyncSender<(u64, String)>,
    output: mpsc::Receiver<(u64, Result<(), &'static str>)>,
}
impl Speech {
    pub fn new() -> Self {
        let (input, requests) = mpsc::sync_channel::<(u64, String)>(1);
        let (results, output) = mpsc::sync_channel(2);
        std::thread::spawn(move || unsafe {
            use windows::Win32::{Media::Speech::*, System::Com::*};
            // Initializing SAPI before the UI message loop starts can block
            // COM activation on that not-yet-pumping thread. Activate lazily.
            let Ok(first) = requests.recv() else {
                return;
            };
            let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let voice = if initialized {
                CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_INPROC_SERVER).ok()
            } else {
                None
            };
            let mut next = Some(first);
            while let Some((generation, text)) = next.take().or_else(|| requests.recv().ok()) {
                let result = if let Some(voice) = &voice {
                    let words = super::util::wide(text);
                    voice
                        .Speak(
                            windows::core::PCWSTR(words.as_ptr()),
                            SPF_DEFAULT.0 as u32,
                            None,
                        )
                        .map_err(|_| "朗读失败，请检查 Windows 语音和音频设备")
                } else {
                    Err("Windows 语音服务不可用，请检查已安装的语音")
                };
                let _ = results.try_send((generation, result));
            }
            drop(voice);
            if initialized {
                CoUninitialize();
            }
        });
        Self { input, output }
    }
    pub fn speak(&self, generation: u64, text: String) -> Result<(), &'static str> {
        if text.is_empty() || text.chars().count() > 16_384 {
            return Err("原文为空或过长，无法朗读");
        }
        self.input
            .try_send((generation, text))
            .map_err(|_| "朗读繁忙，请稍后重试")
    }
    pub fn poll(&self) -> Option<(u64, Result<(), &'static str>)> {
        self.output.try_recv().ok()
    }
}
