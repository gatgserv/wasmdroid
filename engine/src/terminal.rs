//! Bounded terminal display state. Pure parser/serde code: no platform APIs,
//! clipboard, host terminal, file access, URL opening or automatic reporting.
use crate::{EngineError, Result};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU16;

pub const MAX_ROWS: u16 = 128;
pub const MAX_COLS: u16 = 256;
pub const SCROLLBACK_ROWS: usize = 256;
const PAGE_ROWS: usize = 16;
const REPLY_BYTES: usize = 4096;
pub const FRAME_JSON_LIMIT: usize = 256 * 1024;

/// Compact cell tuple: text, foreground, background, attributes, width.
/// Colors: -1 default, 0..255 indexed, 0x1000000|RGB true color.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCell(pub String, pub i32, pub i32, pub u8, pub u8);
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FrameQuery {
    pub token: Option<u64>,
    pub start_row: u16,
    pub scrollback: u16,
    pub known_version: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalPage {
    pub token: u64,
    pub version: u64,
    pub rows: u16,
    pub cols: u16,
    pub start_row: u16,
    pub cells: Vec<Vec<TerminalCell>>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub hide_cursor: bool,
    pub application_cursor: bool,
    pub application_keypad: bool,
    pub bracketed_paste: bool,
    pub alternate_screen: bool,
    pub scrollback: u16,
    pub scrollback_len: u16,
    pub history_discarded: bool,
    pub reply_overflow: bool,
    pub has_more: bool,
}
struct Snapshot {
    page: TerminalPage,
    cells: Vec<Vec<TerminalCell>>,
}
#[derive(Default)]
struct Callbacks {
    replies: Vec<u8>,
    overflow: bool,
    discarded: bool,
}
impl Callbacks {
    fn reply(&mut self, data: &[u8]) {
        if self.replies.len() + data.len() <= REPLY_BYTES {
            self.replies.extend_from_slice(data);
        } else {
            self.overflow = true;
        }
    }
}
impl vt100::Callbacks for Callbacks {
    fn on_scroll(&mut self, _: vt100::capture::RowContents<'_>, _: bool) {
        self.discarded = true;
    }
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        first: Option<u8>,
        second: Option<u8>,
        params: &[&[u16]],
        action: char,
    ) {
        if second.is_some() {
            return;
        }
        let value = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        if let (None, 'n', 5) = (first, action, value) {
            self.reply(b"\x1b[0n");
        }
        if action == 'n' && value == 6 && (first.is_none() || first == Some(b'?')) {
            let (row, col) = screen.cursor_position();
            let prefix = if first.is_some() { "?" } else { "" };
            self.reply(format!("\x1b[{prefix}{};{}R", row + 1, col + 1).as_bytes());
        } else if action == 'c' && value == 0 && first.is_none() {
            self.reply(b"\x1b[?1;2c");
        } else if action == 'c' && first == Some(b'>') {
            self.reply(b"\x1b[>0;1;0c");
        } else if action == 't' && first.is_none() && value == 18 {
            let (rows, cols) = screen.size();
            self.reply(format!("\x1b[8;{};{}t", rows.get(), cols.get()).as_bytes());
        }
    }
    // Title, clipboard, resize and OSC callbacks deliberately retain their
    // no-op defaults. Guest output cannot invoke Android/system capabilities.
}
pub struct Terminal {
    parser: vt100::Parser<Callbacks>,
    version: u64,
    token: u64,
    snapshot: Option<Snapshot>,
}
impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}
fn dimensions(rows: u16, cols: u16) -> Result<(NonZeroU16, NonZeroU16)> {
    if rows == 0 || rows > MAX_ROWS || cols == 0 || cols > MAX_COLS {
        return Err(EngineError::Invalid(
            "terminal dimensions exceed supported bounds".into(),
        ));
    }
    Ok((
        NonZeroU16::new(rows).unwrap(),
        NonZeroU16::new(cols).unwrap(),
    ))
}
fn color(value: vt100::Color) -> i32 {
    match value {
        vt100::Color::Default => -1,
        vt100::Color::Idx(n) => n.into(),
        vt100::Color::Rgb(r, g, b) => 0x1000000 | ((r as i32) << 16) | ((g as i32) << 8) | b as i32,
    }
}
impl Terminal {
    pub fn new(rows: u16, cols: u16) -> Result<Self> {
        let (rows, cols) = dimensions(rows, cols)?;
        Ok(Self {
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                SCROLLBACK_ROWS,
                Callbacks::default(),
            ),
            version: 0,
            token: 0,
            snapshot: None,
        })
    }
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        let (rows, cols) = dimensions(rows, cols)?;
        if self.parser.screen().size() == (rows, cols) {
            return Ok(());
        }
        self.parser.set_size(rows, cols);
        self.version = self.version.saturating_add(1);
        Ok(())
    }
    /// ONLCR affects only the rendered virtual terminal. Raw logs retain the
    /// exact guest bytes. The pinned no-std parser bounds its OSC accumulation.
    pub fn process(&mut self, bytes: &[u8], line_feeds: bool) -> Vec<u8> {
        if bytes.is_empty() {
            return Vec::new();
        }
        if line_feeds {
            let mut start = 0;
            for (index, byte) in bytes.iter().enumerate() {
                if *byte == b'\n' {
                    self.parser.process(&bytes[start..index]);
                    self.parser.process(b"\r\n");
                    start = index + 1;
                }
            }
            self.parser.process(&bytes[start..]);
        } else {
            self.parser.process(bytes);
        }
        self.version = self.version.saturating_add(1);
        std::mem::take(&mut self.parser.callbacks_mut().replies)
    }
    pub fn mark_reply_overflow(&mut self) {
        self.parser.callbacks_mut().overflow = true;
    }
    pub fn page(&mut self, query: &FrameQuery) -> Result<TerminalPage> {
        if query.token.is_none() {
            if query.start_row != 0 {
                return Err(EngineError::Invalid(
                    "new terminal snapshot must start at row zero".into(),
                ));
            }
            self.token = self
                .token
                .checked_add(1)
                .ok_or_else(|| EngineError::Invalid("terminal snapshot token overflow".into()))?;
            let screen = self.parser.screen_mut();
            screen.set_scrollback(query.scrollback.min(SCROLLBACK_ROWS as u16) as usize);
            let (rows, cols) = screen.size();
            let (rows, cols) = (rows.get(), cols.get());
            let mut cells = Vec::with_capacity(rows as usize);
            for row in 0..rows {
                let mut line = Vec::with_capacity(cols as usize);
                for col in 0..cols {
                    let cell = screen.cell(row, col).unwrap();
                    let attributes = (cell.bold() as u8)
                        | ((cell.dim() as u8) << 1)
                        | ((cell.italic() as u8) << 2)
                        | ((cell.underline() as u8) << 3)
                        | ((cell.inverse() as u8) << 4);
                    line.push(TerminalCell(
                        cell.contents().into(),
                        color(cell.fgcolor()),
                        color(cell.bgcolor()),
                        attributes,
                        if cell.is_wide_continuation() {
                            0
                        } else if cell.is_wide() {
                            2
                        } else {
                            1
                        },
                    ));
                }
                cells.push(line);
            }
            let (cursor_row, cursor_col) = screen.cursor_position();
            let mut page = TerminalPage {
                token: self.token,
                version: self.version,
                rows,
                cols,
                start_row: 0,
                cells: Vec::new(),
                cursor_row,
                cursor_col,
                hide_cursor: screen.hide_cursor() || screen.scrollback() > 0,
                application_cursor: screen.application_cursor(),
                application_keypad: screen.application_keypad(),
                bracketed_paste: screen.bracketed_paste(),
                alternate_screen: screen.alternate_screen(),
                scrollback: screen.scrollback() as u16,
                scrollback_len: screen.scrollback_len().min(SCROLLBACK_ROWS) as u16,
                history_discarded: false,
                reply_overflow: false,
                has_more: false,
            };
            screen.set_scrollback(0);
            page.history_discarded = self.parser.callbacks().discarded;
            page.reply_overflow = self.parser.callbacks().overflow;
            self.snapshot = Some(Snapshot { page, cells });
        }
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| EngineError::NotFound("terminal snapshot".into()))?;
        if query
            .token
            .is_some_and(|token| token != snapshot.page.token)
        {
            return Err(EngineError::Conflict(
                "terminal snapshot was replaced".into(),
            ));
        }
        if query.start_row >= snapshot.page.rows {
            return Err(EngineError::Invalid(
                "terminal row is outside snapshot".into(),
            ));
        }
        let mut page = snapshot.page.clone();
        let start = query.start_row as usize;
        let end = (start + PAGE_ROWS).min(snapshot.cells.len());
        page.start_row = query.start_row;
        page.cells = snapshot.cells[start..end].to_vec();
        page.has_more = end < snapshot.cells.len();
        Ok(page)
    }
    pub fn page_json(&mut self, query: &FrameQuery) -> Result<String> {
        if query.token.is_none()
            && query.start_row == 0
            && query.known_version == Some(self.version)
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.version == self.version
                    && snapshot.page.scrollback as usize
                        == (query.scrollback as usize).min(self.parser.screen().scrollback_len())
            })
        {
            return Ok(serde_json::json!({"unchanged":true,"version":self.version}).to_string());
        }
        let json = serde_json::to_string(&self.page(query)?)?;
        if json.len() > FRAME_JSON_LIMIT {
            return Err(EngineError::Invalid(
                "terminal frame exceeds IPC budget".into(),
            ));
        }
        Ok(json)
    }
}
impl TerminalPage {
    pub fn validate(&self) -> Result<()> {
        dimensions(self.rows, self.cols)?;
        if self.start_row >= self.rows
            || !(self.start_row as usize).is_multiple_of(PAGE_ROWS)
            || self.cells.is_empty()
            || self.cells.len() > PAGE_ROWS
            || self.start_row as usize + self.cells.len() > self.rows as usize
            || self.has_more != (self.start_row as usize + self.cells.len() < self.rows as usize)
        {
            return Err(EngineError::Invalid("invalid terminal page bounds".into()));
        }
        for row in &self.cells {
            if row.len() != self.cols as usize {
                return Err(EngineError::Invalid("invalid terminal row width".into()));
            }
            for cell in row {
                if cell.0.len() > 22
                    || cell.3 > 31
                    || cell.4 > 2
                    || [cell.1, cell.2].iter().any(|color| {
                        !(-1..=255).contains(color) && !(0x1000000..=0x1ffffff).contains(color)
                    })
                {
                    return Err(EngineError::Invalid("invalid terminal cell".into()));
                }
            }
        }
        Ok(())
    }
}
