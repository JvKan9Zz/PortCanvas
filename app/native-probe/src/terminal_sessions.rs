//! Native view state. The shared terminal pool remains the owner of PTYs and tmux sessions.
use crate::terminal::Terminal;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Pane {
    pub number: u64,
    pub id: u64,
    pub stream: u64,
    pub phase: String,
    pub message: String,
    pub terminal: Terminal,
    pub draft: String,
    pub editor: crate::terminal_editor::Editor,
    pub sending: bool,
    pub project: String,
    pub project_loaded: bool,
}
pub struct Sessions {
    pub device: String,
    pub active: String,
    pub panes: BTreeMap<String, Pane>,
    pub columns: usize,
    pub rows: usize,
}
impl Default for Sessions {
    fn default() -> Self {
        Self {
            device: String::new(),
            active: String::new(),
            panes: BTreeMap::new(),
            columns: 80,
            rows: 24,
        }
    }
}
impl Sessions {
    pub fn dimensions(&self) -> (usize, usize) {
        (self.columns.clamp(2, 1000), self.rows.clamp(1, 500))
    }
    pub fn sync(&mut self, catalog: &Value) {
        let device = catalog["deviceKey"].as_str().unwrap_or_default();
        if device.is_empty() {
            for pane in self.panes.values_mut() {
                pane.sending = false;
                pane.stream = 0;
                pane.editor.cancel();
                pane.phase = "disconnected".into();
                pane.message = "设备尚未连接。".into();
            }
            return;
        }
        if self.device != device {
            self.panes.clear();
            self.device = device.into();
            self.active.clear();
        }
        let tabs = catalog["tabs"].as_array().cloned().unwrap_or_default();
        self.panes
            .retain(|key, _| tabs.iter().any(|v| v["key"] == key.as_str()));
        let (columns, rows) = self.dimensions();
        for tab in &tabs {
            let Some(key) = tab["key"].as_str() else {
                continue;
            };
            let pane = self.panes.entry(key.into()).or_insert_with(|| Pane {
                number: tab["number"].as_u64().unwrap_or(1),
                id: 0,
                stream: 0,
                phase: String::new(),
                message: String::new(),
                terminal: Terminal::new(columns, rows),
                draft: String::new(),
                editor: Default::default(),
                sending: false,
                project: String::new(),
                project_loaded: false,
            });
            let state = &tab["state"];
            let id = state["id"].as_u64().unwrap_or(0);
            if id != pane.id {
                pane.sending = false;
                pane.editor.cancel();
            }
            if id != pane.stream
                && matches!(state["phase"].as_str(), Some("connecting" | "connected"))
            {
                pane.terminal = Terminal::new(columns, rows);
                pane.stream = id;
            }
            pane.id = id;
            pane.phase = state["phase"].as_str().unwrap_or("disconnected").into();
            pane.message = state["message"].as_str().unwrap_or_default().into();
        }
        if !self.panes.contains_key(&self.active) {
            self.active = tabs
                .first()
                .and_then(|t| t["key"].as_str())
                .unwrap_or_default()
                .into();
        }
    }
    pub fn current(&self) -> Option<&Pane> {
        self.panes.get(&self.active)
    }
    pub fn current_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(&self.active)
    }
    /// Stale packets are still acknowledged by the caller, but never enter a replacement pane.
    pub fn feed(&mut self, device: &str, key: &str, id: u64, bytes: &[u8]) -> Option<Vec<u8>> {
        if self.device != device {
            return None;
        }
        let pane = self.panes.get_mut(key)?;
        if pane.stream == 0 || pane.stream != id {
            return None;
        }
        pane.terminal.feed(bytes);
        Some(pane.terminal.responses())
    }
}

/// Ordinary text goes through the native TextInput's committed-text callback, including IME.
/// Return None for keys that belong to the editor/OS; raw terminal controls return bytes.
pub fn key_bytes(
    terminal: &Terminal,
    text: &str,
    control: bool,
    alt: bool,
    shift: bool,
) -> Option<Vec<u8>> {
    let c = text.chars().next()?;
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
    let cursor = match c {
        '\u{f700}' => Some('A'),
        '\u{f701}' => Some('B'),
        '\u{f702}' => Some('D'),
        '\u{f703}' => Some('C'),
        '\u{f729}' => Some('H'),
        '\u{f72b}' => Some('F'),
        _ => None,
    };
    if let Some(code) = cursor {
        return Some(if modifier == 1 {
            terminal.arrow(code)
        } else {
            format!("\x1b[1;{modifier}{code}").into_bytes()
        });
    }
    let bytes = match c {
        '\u{f727}' => b"\x1b[2~".to_vec(),
        '\u{7f}' => b"\x1b[3~".to_vec(),
        '\u{f72c}' => b"\x1b[5~".to_vec(),
        '\u{f72d}' => b"\x1b[6~".to_vec(),
        '\u{f704}'..='\u{f707}' => vec![27, b'O', b'P' + (c as u32 - 0xf704) as u8],
        '\u{f708}'..='\u{f70f}' => format!(
            "\x1b[{}~",
            [15, 17, 18, 19, 20, 21, 23, 24][c as usize - 0xf708]
        )
        .into_bytes(),
        '\r' | '\n' => vec![13],
        '\u{8}' => vec![127],
        '\u{1b}' => vec![27],
        '\t' => {
            if shift {
                b"\x1b[Z".to_vec()
            } else {
                vec![9]
            }
        }
        'a'..='z' | 'A'..='Z' if control => vec![c.to_ascii_uppercase() as u8 - b'A' + 1],
        ' ' | '@' if control => vec![0],
        '['..='_' if control => vec![c as u8 & 0x1f],
        '?' if control => vec![127],
        _ if alt && !control && !c.is_control() => text.as_bytes().to_vec(),
        _ => return None,
    };
    if alt {
        Some([vec![27], bytes].concat())
    } else {
        Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn catalog(device: &str, id: u64) -> Value {
        json!({"deviceKey":device,"tabs":[{"key":"main","number":1,"state":{"id":id,"phase":"connected"}},
            {"key":"second","number":2,"state":{"id":id+1,"phase":"connected"}}]})
    }
    #[test]
    fn packets_drafts_and_device_changes_are_isolated() {
        let mut s = Sessions::default();
        s.sync(&catalog("a", 2));
        s.current_mut().unwrap().draft = "中文草稿".into();
        s.feed("a", "main", 2, "第一".as_bytes()).unwrap();
        s.feed("a", "second", 3, "第二".as_bytes()).unwrap();
        assert!(s.panes["main"].terminal.text().contains("第一"));
        assert!(!s.panes["main"].terminal.text().contains("第二"));
        assert!(s.feed("a", "main", 1, b"stale").is_none());
        s.sync(&catalog("a", 2));
        assert_eq!(s.current().unwrap().draft, "中文草稿");
        s.sync(&json!({"deviceKey":""}));
        assert_eq!(s.current().unwrap().draft, "中文草稿");
        assert!(s.feed("a", "main", 2, b"late").is_none());
        s.sync(&catalog("b", 2));
        assert!(s.current().unwrap().draft.is_empty());
        assert!(s.feed("a", "main", 2, b"wrong device").is_none());
    }
    #[test]
    fn terminal_keys_use_vt_modes_and_keep_committed_unicode_separate() {
        let mut term = Terminal::new(80, 24);
        assert_eq!(key_bytes(&term, "c", true, false, false).unwrap(), vec![3]);
        assert_eq!(
            key_bytes(&term, "\u{f700}", false, false, false).unwrap(),
            b"\x1b[A"
        );
        term.feed(b"\x1b[?1h");
        assert_eq!(
            key_bytes(&term, "\u{f700}", false, false, false).unwrap(),
            b"\x1bOA"
        );
        assert!(key_bytes(&term, "中文", false, false, false).is_none());
        assert_eq!(key_bytes(&term, "x", false, true, false).unwrap(), b"\x1bx");
        assert_eq!(
            key_bytes(&term, "\u{f700}", true, false, false).unwrap(),
            b"\x1b[1;5A"
        );
    }
    #[test]
    fn queued_final_output_survives_exit_but_never_enters_a_replacement_stream() {
        let mut s = Sessions::default();
        s.sync(&catalog("a", 2));
        let mut ended = catalog("a", 3);
        ended["tabs"][0]["state"]["phase"] = json!("disconnected");
        s.sync(&ended);
        assert!(s.feed("a", "main", 2, b"last output").is_some());
        assert!(s.panes["main"].terminal.text().contains("last output"));
        s.sync(&catalog("a", 4));
        assert!(s.feed("a", "main", 2, b"late").is_none());
        assert!(!s.panes["main"].terminal.text().contains("last output"));
    }
}
