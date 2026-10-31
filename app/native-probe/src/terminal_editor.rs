//! Shell-word completion and per-tab command history. This module never evaluates input.
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub value: String,
    pub cursor: usize,
    pub start: usize,
    pub end: usize,
    pub prefix: String,
    pub commands: bool,
    pub directories: bool,
}
pub fn context(value: &str, cursor: usize) -> Context {
    let cursor = (0..=cursor.min(value.len()))
        .rev()
        .find(|&i| value.is_char_boundary(i))
        .unwrap();
    let (mut quote, mut escaped, mut start, mut word, mut commands, mut head) =
        (None, false, 0, String::new(), true, String::new());
    for (index, c) in value[..cursor].char_indices() {
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        if c == '"' || c == '\'' {
            quote = Some(c);
            continue;
        }
        if c.is_whitespace() || ";|&()<>".contains(c) {
            if !word.is_empty() && commands && !assignment(&word) {
                head = word.clone();
                commands = false;
            }
            word.clear();
            start = index + c.len_utf8();
            if c == '\n' || c == '\r' || ";|&()".contains(c) {
                commands = true;
                head.clear();
            } else if "<>".contains(c) {
                commands = false;
            }
        } else {
            word.push(c);
        }
    }
    let mut end = cursor;
    for (index, c) in value[cursor..].char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c.is_whitespace() || ";|&()<>".contains(c) {
            break;
        }
        end = cursor + index + c.len_utf8();
    }
    Context {
        value: value.into(),
        cursor,
        start,
        end,
        prefix: word,
        commands,
        directories: !commands && matches!(head.as_str(), "cd" | "pushd"),
    }
}
fn assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn quote_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./~-".contains(c))
    {
        return word.into();
    }
    let home = word
        .strip_prefix('~')
        .and_then(|rest| {
            rest.find('/')
                .filter(|&n| {
                    rest[..n]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                })
                .map(|n| n + 2)
        })
        .unwrap_or(0);
    format!("{}'{}'", &word[..home], word[home..].replace('\'', "'\\''"))
}
#[derive(Clone, Debug, Deserialize)]
pub struct Candidate {
    pub value: String,
    pub label: String,
    pub kind: String,
}
#[derive(Deserialize)]
pub struct Reply {
    pub items: Vec<Candidate>,
    pub truncated: bool,
}
fn edit(context: &Context, item: &Candidate, finish: bool) -> (String, usize) {
    let insert = quote_word(&item.value);
    let tail = &context.value[context.end..];
    let space = if finish && item.kind != "directory" && !tail.starts_with(char::is_whitespace) {
        " "
    } else {
        ""
    };
    (
        format!("{}{insert}{space}{tail}", &context.value[..context.start]),
        context.start + insert.len() + space.len(),
    )
}
#[derive(Clone)]
pub struct Request {
    pub serial: u64,
    pub context: Context,
}
pub struct Menu {
    pub context: Context,
    pub items: Vec<Candidate>,
    pub selected: usize,
    pub truncated: bool,
}
#[derive(Default)]
pub struct Editor {
    pub cursor: usize,
    pub history: Vec<String>,
    history_index: usize,
    saved_draft: String,
    serial: u64,
    pub pending: Option<Request>,
    pub menu: Option<Menu>,
}
impl Editor {
    pub fn cancel(&mut self) {
        self.serial = self.serial.wrapping_add(1);
        self.pending = None;
        self.menu = None;
    }
    pub fn changed(&mut self, value: &str, cursor: usize) {
        let cursor = cursor.min(value.len());
        let unchanged = self
            .pending
            .as_ref()
            .map(|r| &r.context)
            .or_else(|| self.menu.as_ref().map(|m| &m.context))
            .is_none_or(|c| c.value == value && c.cursor == cursor);
        if !unchanged {
            self.cancel();
        }
        self.cursor = cursor;
    }
    pub fn begin(&mut self, value: &str, cursor: usize) -> Request {
        self.cancel();
        let request = Request {
            serial: self.serial,
            context: context(value, cursor),
        };
        self.cursor = request.context.cursor;
        self.pending = Some(request.clone());
        request
    }
    /// Returns true only for the still-current request; edits, cursor moves and cancellation invalidate it.
    pub fn complete(&mut self, serial: u64, value: &mut String, reply: Reply) -> bool {
        let Some(request) = self
            .pending
            .as_ref()
            .filter(|r| {
                r.serial == serial && r.context.value == *value && r.context.cursor == self.cursor
            })
            .cloned()
        else {
            return false;
        };
        self.pending = None;
        if reply.items.is_empty() {
            return true;
        }
        if reply.items.len() == 1 {
            (*value, self.cursor) = edit(&request.context, &reply.items[0], true);
            return true;
        }
        let mut prefix = reply.items[0].value.clone();
        for item in &reply.items[1..] {
            while !item.value.starts_with(&prefix) {
                prefix.pop();
            }
        }
        let mut current = request.context;
        if !reply.truncated && prefix.len() > current.prefix.len() {
            (*value, self.cursor) = edit(
                &current,
                &Candidate {
                    value: prefix,
                    label: String::new(),
                    kind: "directory".into(),
                },
                false,
            );
            current = context(value, self.cursor);
        }
        self.menu = Some(Menu {
            context: current,
            items: reply.items,
            selected: 0,
            truncated: reply.truncated,
        });
        true
    }
    pub fn choose(&mut self, value: &mut String, index: usize) -> bool {
        let Some(menu) = self.menu.as_ref().filter(|m| m.context.value == *value) else {
            return false;
        };
        let Some(item) = menu.items.get(index) else {
            return false;
        };
        (*value, self.cursor) = edit(&menu.context, item, true);
        self.cancel();
        true
    }
    pub fn select(&mut self, delta: i32) {
        if let Some(menu) = &mut self.menu {
            menu.selected =
                (menu.selected as i32 + delta).rem_euclid(menu.items.len() as i32) as usize;
        }
    }
    pub fn sent(&mut self, value: &str) {
        if !value.trim().is_empty() {
            self.history.push(value.into());
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
        self.history_index = self.history.len();
        self.saved_draft.clear();
        self.cancel();
    }
    pub fn history(&mut self, value: &mut String, previous: bool) {
        if self.history.is_empty() {
            return;
        }
        self.cancel();
        if self.history_index == self.history.len() {
            self.saved_draft = value.clone();
        }
        self.history_index = if previous {
            self.history_index.saturating_sub(1)
        } else {
            (self.history_index + 1).min(self.history.len())
        };
        *value = if self.history_index == self.history.len() {
            self.saved_draft.clone()
        } else {
            self.history[self.history_index].clone()
        };
        self.cursor = value.len();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(value: &str, kind: &str) -> Candidate {
        Candidate {
            value: value.into(),
            label: value.into(),
            kind: kind.into(),
        }
    }
    #[test]
    fn shell_context_handles_quotes_assignments_redirection_and_mid_word() {
        assert!(context("X=1 py", 6).commands);
        assert!(context("echo x | py", 11).commands);
        assert!(context("cd '中文 目", "cd '中文 目".len()).directories);
        assert_eq!(context("cat a\\ b", 8).prefix, "a b");
        assert!(!context("echo > fi", 9).commands);
        let c = context("cat '中文 文件.txt' rest", "cat '中文".len());
        assert_eq!(c.prefix, "中文");
        assert_eq!(&c.value[c.end..], " rest");
        let (value, cursor) = edit(&c, &candidate("中文 $()'x.txt", "file"), true);
        assert_eq!(value, "cat '中文 $()'\\''x.txt' rest");
        assert_eq!(&value[..cursor], "cat '中文 $()'\\''x.txt'");
    }
    #[test]
    fn completion_coalesces_unicode_prefix_and_preserves_tail_without_executing() {
        let mut editor = Editor::default();
        let mut value = "cat 中ZZ tail".to_string();
        let request = editor.begin(&value, "cat 中".len());
        assert!(editor.complete(
            request.serial,
            &mut value,
            Reply {
                items: vec![
                    candidate("中文甲.txt", "file"),
                    candidate("中文乙.txt", "file")
                ],
                truncated: false
            }
        ));
        assert_eq!(value, "cat '中文' tail");
        editor.select(1);
        assert!(editor.choose(&mut value, 1));
        assert_eq!(value, "cat '中文乙.txt' tail");
        let request = editor.begin("cd ~/", 5);
        value = "cd ~/".into();
        editor.complete(
            request.serial,
            &mut value,
            Reply {
                items: vec![candidate("~/中文 目录/", "directory")],
                truncated: false,
            },
        );
        assert_eq!(value, "cd ~/'中文 目录/'");
    }
    #[test]
    fn late_results_cursor_changes_cancellation_and_truncation_preserve_draft() {
        let mut editor = Editor::default();
        let mut value = "py".to_string();
        let old = editor.begin(&value, 2);
        let new = editor.begin(&value, 2);
        assert!(!editor.complete(
            old.serial,
            &mut value,
            Reply {
                items: vec![candidate("old", "command")],
                truncated: false
            }
        ));
        editor.changed(&value, 1);
        assert!(!editor.complete(
            new.serial,
            &mut value,
            Reply {
                items: vec![],
                truncated: false
            }
        ));
        let request = editor.begin(&value, 2);
        editor.complete(
            request.serial,
            &mut value,
            Reply {
                items: vec![
                    candidate("python", "command"),
                    candidate("python3", "command"),
                ],
                truncated: true,
            },
        );
        assert_eq!(value, "py");
        editor.cancel();
        assert!(!editor.choose(&mut value, 0));
        assert_eq!(value, "py");
    }
    #[test]
    fn history_is_bounded_and_restores_each_editors_unsent_unicode_draft() {
        let mut a = Editor::default();
        let mut b = Editor::default();
        for n in 0..105 {
            a.sent(&format!("command-{n}"));
        }
        b.sent("other");
        assert_eq!(a.history.len(), 100);
        assert_eq!(a.history[0], "command-5");
        let mut draft = "未发送草稿".into();
        a.history(&mut draft, true);
        assert_eq!(draft, "command-104");
        a.history(&mut draft, false);
        assert_eq!(draft, "未发送草稿");
        b.history(&mut draft, true);
        assert_eq!(draft, "other");
    }
}
