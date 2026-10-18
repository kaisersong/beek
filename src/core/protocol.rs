//! Bounded, versioned messages. Selection content belongs only in private IPC,
//! never diagnostic logs, command-line arguments or persistent evidence.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Read, Write};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 256 * 1024;
pub const MAX_CHARS: usize = 16_384;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub hwnd: u64,
    pub pid: u32,
    pub created: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && n.abs() < 10_000_000.)
            && self.width > 0.
            && self.height > 0.
    }
    pub fn clamp_popup(self, width: f64, height: f64, work: Self) -> Self {
        let width = width.max(1.).min(work.width);
        let height = height.max(1.).min(work.height);
        Self {
            x: self.x.max(work.x).min(work.x + work.width - width),
            y: (self.y + self.height + 6.)
                .max(work.y)
                .min(work.y + work.height - height),
            width,
            height,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub source: Source,
    pub epoch: u64,
    pub runtime_id: Vec<i32>,
    pub text: String,
    pub rects: Vec<Rect>,
    pub clipboard_sequence: Option<u32>,
}
impl Selection {
    pub fn validate(&self) -> bool {
        self.source.pid != 0
            && self.source.created != 0
            && self.runtime_id.len() <= 128
            && !self.text.trim().is_empty()
            && self.text.chars().count() <= MAX_CHARS
            && self.rects.len() <= 1024
            && self.rects.iter().all(|r| r.valid())
            && (self.clipboard_sequence.is_some() || !self.rects.is_empty())
    }
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.text.as_bytes()).into()
    }
    pub fn same_selection(&self, fresh: &Self) -> bool {
        self.source == fresh.source
            && self.epoch == fresh.epoch
            && self.runtime_id == fresh.runtime_id
            && self.clipboard_sequence == fresh.clipboard_sequence
            && self.digest() == fresh.digest()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedBuffer {
    pub handle: u64,
    pub bytes: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub version: u32,
    pub nonce: String,
    pub id: u64,
    pub source: Source,
    pub epoch: u64,
    pub point: [i32; 2],
    pub pointer_anchor: bool,
    // Optional second endpoint of the same fresh, same-window mouse gesture.
    // Main/helper build hashes require the exact same schema on both sides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_end: Option<[i32; 2]>,
    pub clipboard: bool,
    pub explicit_clipboard: bool,
    pub write_buffer: Option<SharedBuffer>,
    pub clipboard_sequence: Option<u32>,
}
impl Query {
    pub fn pointer_points(&self) -> impl Iterator<Item = [i32; 2]> {
        std::iter::once(self.point).chain(self.pointer_end)
    }
    pub fn touches_selection(&self, rects: &[Rect]) -> bool {
        self.pointer_points().any(|p| {
            rects.iter().any(|r| {
                p[0] as f64 >= r.x - 20.
                    && p[0] as f64 <= r.x + r.width + 20.
                    && p[1] as f64 >= r.y - 20.
                    && p[1] as f64 <= r.y + r.height + 20.
            })
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u32,
    pub build: String,
    pub nonce: String,
    pub id: u64,
    pub source: Source,
    pub selection: Option<Selection>,
    pub status: Status,
    pub clipboard_sequence: Option<u32>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum Status {
    Ready,
    Selected,
    Empty,
    Unsupported,
    TooLong,
    Protected,
    SourceChanged,
    Busy,
    Failed,
    Alive,
    Changed,
    ClipboardWritten,
}

pub fn read_frame<T: for<'a> Deserialize<'a>>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut line = Vec::with_capacity(4096);
    // Limit WHILE reading, not after an unbounded allocation.
    reader
        .take((MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut line)?;
    if line.len() > MAX_FRAME || line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid IPC frame size or termination",
        ));
    }
    serde_json::from_slice(&line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC schema"))
}
pub fn write_frame(value: &impl Serialize, writer: &mut impl Write) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| io::Error::other("IPC serialization"))?;
    if bytes.len() >= MAX_FRAME {
        return Err(io::Error::other("IPC frame too large"));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_and_unterminated_input_is_rejected_before_parsing() {
        let mut huge = io::Cursor::new(vec![b'x'; MAX_FRAME * 3]);
        assert!(read_frame::<serde_json::Value>(&mut huge).is_err());
        assert_eq!(huge.position(), (MAX_FRAME + 1) as u64);
        assert!(read_frame::<serde_json::Value>(&mut io::Cursor::new(b"{}")).is_err());
    }
    #[test]
    fn drag_may_start_or_end_in_blank_space_but_must_touch_the_selection() {
        let mut q = Query {
            version: VERSION,
            nonce: "fixture".into(),
            id: 1,
            source: Source {
                hwnd: 1,
                pid: 1,
                created: 1,
                generation: 1,
            },
            epoch: 1,
            point: [-800, 110],
            pointer_anchor: true,
            pointer_end: Some([-450, 110]),
            clipboard: false,
            explicit_clipboard: false,
            write_buffer: None,
            clipboard_sequence: None,
        };
        let rects = [Rect {
            x: -500.,
            y: 100.,
            width: 100.,
            height: 20.,
        }];
        assert!(q.touches_selection(&rects)); // start in blank, finish on text
        q.point = [-450, 110];
        q.pointer_end = Some([-800, 110]);
        assert!(q.touches_selection(&rects)); // start on text, finish in blank
        q.point = [-800, 110];
        q.pointer_end = Some([-700, 110]);
        assert!(!q.touches_selection(&rects)); // neither endpoint touches the range
        q.pointer_end = None;
        assert!(!q.touches_selection(&rects));
    }
    #[test]
    fn geometry_preserves_negative_monitor_coordinates() {
        let work = Rect {
            x: -1920.,
            y: -300.,
            width: 1920.,
            height: 1080.,
        };
        let p = Rect {
            x: -4.,
            y: 770.,
            width: 4.,
            height: 12.,
        }
        .clamp_popup(480., 220., work);
        assert_eq!(p.x, -480.);
        assert_eq!(p.y, 560.);
        assert!(p.valid());
    }
}
