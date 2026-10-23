use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub kind: String,
    pub shell: String,
    pub sequence: String,
    pub code: Option<i32>,
    pub command: String,
    pub cwd: String,
    pub project_root: String,
    pub end_cwd: String,
    pub background_jobs: Vec<String>,
}
fn digits(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}
pub fn packet(text: &str) -> Option<Marker> {
    if text.len() > 150000 {
        return None;
    }
    let p = text.split(';').collect::<Vec<_>>();
    if p.len() != 9
        || !["ready", "begin", "end"].contains(&p[0])
        || !p[1]
            .split_once('-')
            .is_some_and(|(a, b)| digits(a, 1, 150000) && digits(b, 1, 150000))
        || !digits(p[2], 1, 12)
    {
        return None;
    }
    let code = if p[0] == "end" {
        if !digits(p[3], 1, 3) {
            return None;
        }
        Some(p[3].parse().ok()?)
    } else {
        None
    };
    let decode = |value: &str| -> Option<String> {
        Some(String::from_utf8_lossy(&STANDARD.decode(value).ok()?).into_owned())
    };
    Some(Marker {
        kind: p[0].into(),
        shell: p[1].into(),
        sequence: p[2].into(),
        code,
        command: decode(p[4])?,
        cwd: decode(p[5])?,
        project_root: decode(p[6])?,
        end_cwd: decode(p[7])?,
        background_jobs: p[8]
            .split(',')
            .filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
            .map(String::from)
            .collect(),
    })
}
pub fn control_output(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut n = 0;
    while n < bytes.len() {
        if bytes[n] == b'\\'
            && n + 3 < bytes.len()
            && bytes[n + 1..n + 4]
                .iter()
                .all(|c| (b'0'..=b'7').contains(c))
        {
            result.push(
                ((bytes[n + 1] - b'0') as u16 * 64
                    + (bytes[n + 2] - b'0') as u16 * 8
                    + (bytes[n + 3] - b'0') as u16) as u8,
            );
            n += 4;
        } else {
            result.push(bytes[n]);
            n += 1;
        }
    }
    result
}
#[derive(Clone, Debug)]
pub enum Event {
    Text(String),
    Marker(Marker),
}
#[derive(Clone, Copy)]
enum Mode {
    Text,
    Escape,
    Csi,
    Charset,
    Osc,
    OscEscape,
    Discard,
    DiscardEscape,
}
pub struct OutputParser {
    mode: Mode,
    line: String,
    osc: String,
    cr: bool,
    utf8: Vec<u8>,
}
impl Default for OutputParser {
    fn default() -> Self {
        Self {
            mode: Mode::Text,
            line: String::new(),
            osc: String::new(),
            cr: false,
            utf8: vec![],
        }
    }
}
impl OutputParser {
    pub fn push(&mut self, input: &[u8]) -> Vec<Event> {
        let mut bytes = std::mem::take(&mut self.utf8);
        bytes.extend_from_slice(input);
        let mut text = String::new();
        let mut offset = 0;
        while offset < bytes.len() {
            match std::str::from_utf8(&bytes[offset..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    break;
                }
                Err(error) => {
                    let end = offset + error.valid_up_to();
                    text.push_str(std::str::from_utf8(&bytes[offset..end]).unwrap());
                    if let Some(size) = error.error_len() {
                        text.push('�');
                        offset = end + size;
                    } else {
                        self.utf8.extend_from_slice(&bytes[end..]);
                        break;
                    }
                }
            }
        }
        self.consume(&text)
    }
    pub fn flush(&mut self) -> Vec<Event> {
        let bytes = std::mem::take(&mut self.utf8);
        let mut events = self.consume(&String::from_utf8_lossy(&bytes));
        self.line(false, &mut events);
        events
    }
    fn line(&mut self, newline: bool, events: &mut Vec<Event>) {
        if !self.line.is_empty() || newline {
            let mut text = std::mem::take(&mut self.line);
            if newline {
                text.push('\n');
            }
            events.push(Event::Text(text));
        }
        self.cr = false;
    }
    fn consume(&mut self, text: &str) -> Vec<Event> {
        let mut events = Vec::new();
        for ch in text.chars() {
            match self.mode {
                Mode::Osc | Mode::OscEscape => {
                    if ch == '\x07' || matches!(self.mode, Mode::OscEscape) && ch == '\\' {
                        if let Some(body) = self.osc.strip_prefix("777;WCSLOG;") {
                            if let Some(marker) = packet(body) {
                                self.line(false, &mut events);
                                events.push(Event::Marker(marker));
                            }
                        }
                        self.osc.clear();
                        self.mode = Mode::Text;
                        continue;
                    }
                    if ch == '\x1b' {
                        self.mode = Mode::OscEscape;
                        continue;
                    }
                    if self.osc.len() < 150032 {
                        self.osc.push(ch);
                        self.mode = Mode::Osc;
                    } else {
                        self.osc.clear();
                        self.mode = Mode::Discard;
                    }
                    continue;
                }
                Mode::Discard => {
                    if ch == '\x07' {
                        self.mode = Mode::Text;
                    } else if ch == '\x1b' {
                        self.mode = Mode::DiscardEscape;
                    }
                    continue;
                }
                Mode::DiscardEscape => {
                    self.mode = if ch == '\\' {
                        Mode::Text
                    } else {
                        Mode::Discard
                    };
                    continue;
                }
                Mode::Csi => {
                    if ('\x40'..='\x7e').contains(&ch) {
                        self.mode = Mode::Text;
                    }
                    continue;
                }
                Mode::Charset => {
                    self.mode = Mode::Text;
                    continue;
                }
                Mode::Escape => {
                    self.mode = match ch {
                        '[' => Mode::Csi,
                        ']' => Mode::Osc,
                        'P' | '^' | '_' => Mode::Discard,
                        '(' | ')' | '*' | '+' => Mode::Charset,
                        _ => Mode::Text,
                    };
                    continue;
                }
                Mode::Text => {}
            }
            if ch == '\x1b' {
                self.mode = Mode::Escape;
                continue;
            }
            if ch == '\r' {
                self.cr = true;
                continue;
            }
            if ch == '\n' {
                self.line(true, &mut events);
                continue;
            }
            if self.cr {
                self.line(true, &mut events);
            }
            if ch == '\x08' {
                self.line.pop();
                continue;
            }
            if ch == '\t' || ch >= ' ' {
                self.line.push(ch);
                if self.line.len() >= 65536 {
                    self.line(false, &mut events);
                }
            }
        }
        events
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn matches_legacy_packets_and_chunked_chinese_stream() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/protocol.json")).unwrap();
        for (index, p) in fixture["valid"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                serde_json::to_value(packet(p.as_str().unwrap()).unwrap()).unwrap(),
                fixture["parsed"][index]
            );
        }
        for p in fixture["invalid"].as_array().unwrap() {
            assert!(packet(p.as_str().unwrap()).is_none());
        }
        let bytes = STANDARD
            .decode(fixture["stream"].as_str().unwrap())
            .unwrap();
        let mut parser = OutputParser::default();
        let mut events = Vec::new();
        for byte in bytes {
            events.extend(parser.push(&[byte]));
        }
        events.extend(parser.flush());
        let text = events
            .iter()
            .filter_map(|e| {
                if let Event::Text(s) = e {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect::<String>();
        let markers = events
            .into_iter()
            .filter_map(|e| {
                if let Event::Marker(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(text, fixture["text"].as_str().unwrap());
        assert_eq!(serde_json::to_value(markers).unwrap(), fixture["markers"]);
    }
    #[test]
    fn control_mode_escapes_and_bounded_lines() {
        assert_eq!(
            String::from_utf8(control_output("中文\\015\\012\\134x".as_bytes())).unwrap(),
            "中文\r\n\\x"
        );
        let mut parser = OutputParser::default();
        let text = "中".repeat(100000);
        let mut events = parser.push(text.as_bytes());
        events.extend(parser.flush());
        let output = events
            .into_iter()
            .filter_map(|e| {
                if let Event::Text(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
            .collect::<String>();
        assert_eq!(output, text);
    }
}
