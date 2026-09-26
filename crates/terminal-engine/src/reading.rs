//! Content anchors locate text only within one immutable, bounded terminal view.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextLine {
    pub text: String,
    pub wrapped: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadView {
    pub epoch: u64,
    pub revision: u64,
    pub dimensions_epoch: u64,
    pub alternate_screen: bool,
    pub screen_start: usize,
    pub source_partial: bool,
    pub lines: Vec<TextLine>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Tail,
    Search,
    Screen,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadOptions {
    #[serde(default = "default_head_lines")]
    pub head_lines: usize,
    #[serde(default = "default_tail_lines")]
    pub tail_lines: usize,
    #[serde(default)]
    pub tui_lines: Vec<String>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_lines")]
    pub max_lines: usize,
    #[serde(default = "default_bytes")]
    pub max_bytes: usize,
    pub start_before: Option<Vec<String>>,
    pub stop_before: Option<Vec<String>>,
}
pub fn default_head_lines() -> usize {
    10
}
pub fn default_tail_lines() -> usize {
    20
}
pub const MAX_ANCHOR_LINES: usize = 100;
fn default_lines() -> usize {
    200
}
fn default_bytes() -> usize {
    64 * 1024
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            head_lines: default_head_lines(),
            tail_lines: default_tail_lines(),
            tui_lines: vec![],
            mode: Mode::Tail,
            max_lines: default_lines(),
            max_bytes: default_bytes(),
            start_before: None,
            stop_before: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Anchor {
    pub lines: Vec<String>,
    /// Positions are private view-local provenance, never live terminal row IDs.
    pub positions: Vec<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlankRun {
    pub offset: usize,
    pub count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineFragment {
    pub view_line: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub full_bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadSlice {
    pub head_lines: usize,
    pub tail_lines: usize,
    pub body: String,
    pub start: usize,
    pub end: usize,
    pub head: Anchor,
    pub tail: Anchor,
    pub blank_runs: Vec<BlankRun>,
    pub physical_lines: usize,
    pub content_lines: usize,
    pub end_reason: String,
    pub boundary_status: String,
    pub older_available: bool,
    pub source_partial: bool,
    pub anchor_incomplete: bool,
    pub fragment: Option<LineFragment>,
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ReadError {
    #[error("start_anchor_required")]
    StartRequired,
    #[error("invalid_anchor")]
    InvalidAnchor,
    #[error("anchor_unavailable_tui_only")]
    TuiOnly,
    #[error("invalid_read_options")]
    InvalidOptions,
    #[error("start_anchor_not_found")]
    StartNotFound,
    #[error("ambiguous_anchor: {0:?}")]
    Ambiguous(Vec<(usize, usize)>),
    #[error("invalid_boundary_order")]
    InvalidOrder,
    #[error("scan_limit")]
    ScanLimit,
}
const SCAN_LIMIT: usize = 12_000;
fn validate_anchor(lines: &[String]) -> Result<(), ReadError> {
    if lines.is_empty()
        || lines.len() > MAX_ANCHOR_LINES
        || lines.iter().all(|s| s.trim().is_empty())
        || lines.iter().map(String::len).sum::<usize>() > 64 * 1024
    {
        return Err(ReadError::InvalidAnchor);
    }
    Ok(())
}
/// TUI classification is explicit; removing it must never create an empty wildcard.
pub fn filter_anchor(lines: &[String], tui_lines: &[String]) -> Result<Vec<String>, ReadError> {
    validate_anchor(lines)?;
    let filtered = lines
        .iter()
        .filter(|line| !line.trim().is_empty() && !tui_lines.contains(line))
        .cloned()
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        return Err(ReadError::TuiOnly);
    }
    Ok(filtered)
}
impl ReadView {
    fn locate(
        &self,
        anchor: &[String],
        tui_lines: &[String],
    ) -> Result<Option<(usize, usize)>, ReadError> {
        validate_anchor(anchor)?;
        let wanted: Vec<_> = anchor.iter().filter(|s| !s.trim().is_empty()).collect();
        if self.lines.len() > SCAN_LIMIT {
            return Err(ReadError::ScanLimit);
        }
        let content: Vec<_> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.text.trim().is_empty() && !tui_lines.contains(&l.text))
            .collect();
        let mut matches = Vec::new();
        for block in content.windows(wanted.len()) {
            if block
                .iter()
                .zip(&wanted)
                .all(|((_, line), w)| line.text == w.as_str())
            {
                matches.push((block[0].0, block[block.len() - 1].0 + 1));
                if matches.len() == 8 {
                    break;
                }
            }
        }
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => Err(ReadError::Ambiguous(matches)),
        }
    }
    pub fn read(&self, options: &ReadOptions) -> Result<ReadSlice, ReadError> {
        self.read_with_provenance(options, None, None)
    }
    /// Provenance is supplied only by Host after validating record/view identity.
    /// The range excludes the whole adjacent record, including its edge blanks.
    pub fn read_with_provenance(
        &self,
        o: &ReadOptions,
        start: Option<(usize, usize)>,
        stop: Option<(usize, usize)>,
    ) -> Result<ReadSlice, ReadError> {
        if !(1..=1000).contains(&o.max_lines)
            || !(1..=1024 * 1024).contains(&o.max_bytes)
            || !(1..=MAX_ANCHOR_LINES).contains(&o.head_lines)
            || !(1..=MAX_ANCHOR_LINES).contains(&o.tail_lines)
            || o.tui_lines.len() > 1000
            || o.tui_lines.iter().map(String::len).sum::<usize>() > 64 * 1024
        {
            return Err(ReadError::InvalidOptions);
        }
        let mut o = o.clone();
        for anchor in [&mut o.start_before, &mut o.stop_before]
            .into_iter()
            .flatten()
        {
            if self.alternate_screen {
                return Err(ReadError::TuiOnly);
            }
            *anchor = filter_anchor(anchor, &o.tui_lines)?;
        }
        match o.mode {
            Mode::Search if o.start_before.is_none() => return Err(ReadError::StartRequired),
            Mode::Tail if o.start_before.is_some() => return Err(ReadError::InvalidOptions),
            Mode::Screen if o.start_before.is_some() || o.stop_before.is_some() => {
                return Err(ReadError::InvalidOptions);
            }
            _ => {}
        }
        for a in [&o.start_before, &o.stop_before].into_iter().flatten() {
            validate_anchor(a)?;
        }
        let checked = |range: (usize, usize)| -> Result<(usize, usize), ReadError> {
            if range.0 > range.1 || range.1 > self.lines.len() {
                Err(ReadError::InvalidOptions)
            } else {
                Ok(range)
            }
        };
        let start = match &o.start_before {
            Some(a) => Some(match start {
                Some(p) => checked(p)?,
                None => self
                    .locate(a, &o.tui_lines)?
                    .ok_or(ReadError::StartNotFound)?,
            }),
            None => None,
        };
        let stop = match &o.stop_before {
            Some(a) => match stop {
                Some(p) => Some(checked(p)?),
                None => self.locate(a, &o.tui_lines)?,
            },
            None => None,
        };
        let end = start.map_or(self.lines.len(), |r| r.0);
        let mut floor = if o.mode == Mode::Screen {
            self.screen_start
        } else {
            0
        };
        if let Some(p) = stop {
            if start == Some(p) {
                floor = end;
            } else if p.1 > end {
                return Err(ReadError::InvalidOrder);
            } else {
                floor = p.1;
            }
        }
        let mut begin = end;
        let mut count = 0;
        let mut bytes = 0;
        let mut reason = if stop.is_some() {
            "stop_anchor"
        } else {
            "source_start"
        };
        while begin > floor {
            if end - begin >= SCAN_LIMIT {
                reason = "scan_limit";
                break;
            }
            let text = &self.lines[begin - 1].text;
            if !text.trim().is_empty() && count >= o.max_lines {
                reason = "line_limit";
                break;
            }
            if bytes + text.len() + usize::from(begin < end) > o.max_bytes {
                reason = "byte_limit";
                break;
            }
            bytes += text.len() + usize::from(begin < end);
            count += usize::from(!text.trim().is_empty());
            begin -= 1;
        }
        let selected = &self.lines[begin..end];
        // An oversized bottom line must make bounded progress without becoming an anchor.
        // Its complete source remains in the immutable view for Host archival/paging.
        let fragment = if begin == end && begin > floor && reason == "byte_limit" {
            let line = &self.lines[begin - 1].text;
            let mut byte_start = line.len().saturating_sub(o.max_bytes);
            while byte_start < line.len() && !line.is_char_boundary(byte_start) {
                byte_start += 1;
            }
            Some(LineFragment {
                view_line: begin - 1,
                byte_start,
                byte_end: line.len(),
                full_bytes: line.len(),
            })
        } else {
            None
        };
        let anchor = |reverse: bool| {
            let positions: Vec<_> = (begin..end)
                .filter(|&i| !self.lines[i].text.trim().is_empty())
                .collect();
            let positions = if reverse {
                positions
                    .into_iter()
                    .rev()
                    .take(o.tail_lines)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
            } else {
                positions.into_iter().take(o.head_lines).collect()
            };
            Anchor {
                lines: positions
                    .iter()
                    .map(|&i| self.lines[i].text.clone())
                    .collect(),
                positions,
            }
        };
        let mut blanks: Vec<BlankRun> = Vec::new();
        for (offset, line) in selected.iter().enumerate() {
            if line.text.trim().is_empty() {
                if let Some(last) = blanks.last_mut().filter(|r| r.offset + r.count == offset) {
                    last.count += 1;
                } else {
                    blanks.push(BlankRun { offset, count: 1 });
                }
            }
        }
        let body = fragment.as_ref().map_or_else(
            || {
                selected
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            |f| self.lines[f.view_line].text[f.byte_start..f.byte_end].to_owned(),
        );
        Ok(ReadSlice {
            head_lines: o.head_lines,
            tail_lines: o.tail_lines,
            body,
            start: begin,
            end,
            head: anchor(false),
            tail: anchor(true),
            blank_runs: blanks,
            physical_lines: selected.len() + usize::from(fragment.is_some()),
            content_lines: count + usize::from(fragment.is_some()),
            end_reason: reason.into(),
            boundary_status: if o.stop_before.is_none() {
                "not_requested"
            } else if reason == "stop_anchor" {
                "matched"
            } else {
                "not_found_in_window"
            }
            .into(),
            older_available: begin > floor,
            source_partial: self.source_partial,
            anchor_incomplete: count < o.head_lines || count < o.tail_lines,
            fragment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    fn view() -> ReadView {
        ReadView {
            epoch: 1,
            revision: 1,
            dimensions_epoch: 1,
            alternate_screen: false,
            screen_start: 1180,
            source_partial: false,
            lines: (0..1200)
                .map(|n| TextLine {
                    text: format!("line-{n}"),
                    wrapped: false,
                })
                .collect(),
        }
    }
    #[test]
    fn tail_search_and_exclusive_boundaries() {
        let v = view();
        let a = v.read(&ReadOptions::default()).unwrap();
        assert_eq!(a.content_lines, 200);
        assert_eq!(a.head.lines[0], "line-1000");
        let b = v
            .read(&ReadOptions {
                mode: Mode::Search,
                max_lines: 1000,
                start_before: Some(a.head.lines),
                stop_before: Some(vec!["line-30".into()]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(b.content_lines, 969);
        assert_eq!(b.end_reason, "stop_anchor");
        assert_eq!(b.head.lines[0], "line-31");
        assert_eq!(b.tail.lines.last().unwrap(), "line-999");
        assert_eq!(
            v.read(&ReadOptions {
                mode: Mode::Search,
                ..Default::default()
            })
            .unwrap_err(),
            ReadError::StartRequired
        );
        assert_eq!(
            v.read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(vec![" \t".into()]),
                ..Default::default()
            })
            .unwrap_err(),
            ReadError::InvalidAnchor
        );
    }
    #[test]
    fn duplicate_missing_inverted_and_blank_anchors() {
        let mut v = view();
        v.lines[50].text = "line-10".into();
        assert!(matches!(
            v.read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(vec!["line-10".into()]),
                ..Default::default()
            }),
            Err(ReadError::Ambiguous(_))
        ));
        assert_eq!(
            v.read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(vec!["missing".into()]),
                ..Default::default()
            })
            .unwrap_err(),
            ReadError::StartNotFound
        );
        assert_eq!(
            v.read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(vec!["line-20".into()]),
                stop_before: Some(vec!["line-30".into()]),
                ..Default::default()
            })
            .unwrap_err(),
            ReadError::InvalidOrder
        );
        v.lines.iter_mut().for_each(|l| l.text.clear());
        let s = v.read(&ReadOptions::default()).unwrap();
        assert!(s.head.lines.is_empty());
        assert_eq!(s.blank_runs[0].count, 1200);
    }
    #[test]
    fn authority_view_is_immutable_and_hides_hidden_cells() {
        let mut e = Engine::new(3, 12, 1).unwrap();
        e.feed("中文e\u{301}\r\n\x1b[8msecret\x1b[0m\r\nlast".as_bytes());
        let v = e.read_view(12000, 1024 * 1024);
        let original = v.read(&ReadOptions::default()).unwrap().body;
        assert!(original.contains("中文e\u{301}"));
        assert!(!original.contains("secret"));
        e.feed(b"\x1b[?1049hTUI");
        let alt = e.read_view(12000, 1024 * 1024);
        assert!(alt.alternate_screen);
        assert!(
            !alt.read(&ReadOptions::default())
                .unwrap()
                .body
                .contains("last")
        );
        e.resize(5, 20).unwrap();
        e.feed(b"\x1b[3J\x1b[2J");
        assert_eq!(v.read(&ReadOptions::default()).unwrap().body, original);
    }
    #[test]
    fn blank_edges_adjacent_reads_and_utf8_byte_limit() {
        let mut v = view();
        v.lines[999].text.clear();
        v.lines[1000].text.clear();
        let a = v
            .read(&ReadOptions {
                max_lines: 199,
                ..Default::default()
            })
            .unwrap();
        let b = v
            .read_with_provenance(
                &ReadOptions {
                    mode: Mode::Search,
                    max_lines: 1000,
                    start_before: Some(a.head.lines.clone()),
                    ..Default::default()
                },
                Some((a.start, a.end)),
                None,
            )
            .unwrap();
        assert_eq!(b.end, a.start);
        assert_eq!(b.physical_lines + a.physical_lines, v.lines.len());
        let last = v.lines.last_mut().unwrap();
        last.text = "中".repeat(100);
        let s = v
            .read(&ReadOptions {
                max_bytes: 11,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(s.end_reason, "byte_limit");
        assert!(s.body.len() <= 11);
        assert!(s.fragment.is_some());
        assert!(s.tail.lines.is_empty());
    }
    #[test]
    fn missing_stop_and_same_boundaries_are_explicit() {
        let v = view();
        let s = v
            .read(&ReadOptions {
                stop_before: Some(vec!["absent".into()]),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(s.boundary_status, "not_found_in_window");
        assert_eq!(s.end_reason, "line_limit");
        let s = v
            .read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(vec!["line-500".into()]),
                stop_before: Some(vec!["line-500".into()]),
                ..Default::default()
            })
            .unwrap();
        assert!(s.body.is_empty());
        assert_eq!(s.end_reason, "stop_anchor");
    }
    #[test]
    fn progress_repaint_wrap_and_capture_limits() {
        let mut e = Engine::new(3, 8, 1).unwrap();
        e.feed(
            b"abcdefghijkl
10%\r90%\r100%",
        );
        let v = e.read_view(100, 1024);
        assert!(v.lines.iter().any(|l| l.wrapped));
        let before = e.snapshot();
        let limited = e.read_view(1, 1024);
        assert!(limited.source_partial);
        assert_eq!(limited.lines.len(), 1);
        assert_eq!(before, e.snapshot());
        e.feed(b"\x1b[2J\x1b[Hnew");
        assert!(
            v.read(&ReadOptions::default())
                .unwrap()
                .body
                .contains("100%")
        );
    }
}

#[cfg(test)]
mod tui_contracts {
    use super::*;
    fn view(lines: &[&str]) -> ReadView {
        ReadView {
            epoch: 1,
            revision: 1,
            dimensions_epoch: 1,
            alternate_screen: false,
            screen_start: 0,
            source_partial: false,
            lines: lines
                .iter()
                .map(|s| TextLine {
                    text: (*s).into(),
                    wrapped: false,
                })
                .collect(),
        }
    }
    #[test]
    fn display_edges_default_to_ten_and_twenty_and_accept_custom_counts() {
        let strings = (0..60).map(|n| format!("log-{n}")).collect::<Vec<_>>();
        let borrowed = strings.iter().map(String::as_str).collect::<Vec<_>>();
        let view = view(&borrowed);
        let record = view.read(&ReadOptions::default()).unwrap();
        assert_eq!(record.head.lines.len(), 10);
        assert_eq!(record.tail.lines.len(), 20);
        assert_eq!(record.tail.lines[0], "log-40");
        let older = view
            .read(&ReadOptions {
                mode: Mode::Search,
                start_before: Some(record.tail.lines),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(older.body.lines().last(), Some("log-39"));
        let custom = view
            .read(&ReadOptions {
                head_lines: 3,
                tail_lines: 7,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(custom.head.lines, strings[..3]);
        assert_eq!(custom.tail.lines, strings[53..]);
        for count in [0, 101] {
            assert!(matches!(
                view.read(&ReadOptions {
                    tail_lines: count,
                    ..Default::default()
                }),
                Err(ReadError::InvalidOptions)
            ));
        }
    }
    #[test]
    fn changed_tui_is_removed_from_both_anchor_and_source_without_changing_raw_text() {
        let view = view(&[
            "older log",
            "job start",
            "spinner 2",
            "job end",
            "status 99",
        ]);
        let before = view.read(&ReadOptions::default()).unwrap().body;
        let options = ReadOptions {
            mode: Mode::Search,
            start_before: Some(vec![
                "job start".into(),
                "spinner 1".into(),
                "job end".into(),
                "status 12".into(),
            ]),
            tui_lines: vec![
                "spinner 1".into(),
                "spinner 2".into(),
                "status 12".into(),
                "status 99".into(),
            ],
            ..Default::default()
        };
        let found = view.read(&options).unwrap();
        assert_eq!(found.body, "older log");
        assert_eq!(view.read(&ReadOptions::default()).unwrap().body, before);
        let tui_only = ReadOptions {
            mode: Mode::Search,
            start_before: Some(vec!["spinner 1".into()]),
            tui_lines: options.tui_lines,
            ..Default::default()
        };
        assert_eq!(view.read(&tui_only).unwrap_err(), ReadError::TuiOnly);
        assert_eq!(
            view.read_with_provenance(&tui_only, Some((1, 3)), None)
                .unwrap_err(),
            ReadError::TuiOnly
        );
        let stop = ReadOptions {
            stop_before: Some(vec!["status 99".into()]),
            tui_lines: vec!["status 99".into()],
            ..Default::default()
        };
        assert_eq!(view.read(&stop).unwrap_err(), ReadError::TuiOnly);
    }
    #[test]
    fn tui_filter_does_not_resolve_ambiguous_logs_or_search_alternate_screen() {
        let mut view = view(&["same", "spinner 1", "log", "same", "spinner 2", "log"]);
        let options = ReadOptions {
            mode: Mode::Search,
            start_before: Some(vec!["same".into(), "spinner 1".into(), "log".into()]),
            tui_lines: vec!["spinner 1".into(), "spinner 2".into()],
            ..Default::default()
        };
        assert!(matches!(view.read(&options),Err(ReadError::Ambiguous(ranges)) if ranges.len()==2));
        view.alternate_screen = true;
        assert_eq!(view.read(&options).unwrap_err(), ReadError::TuiOnly);
        assert!(
            view.read(&ReadOptions::default())
                .unwrap()
                .body
                .contains("spinner 2")
        );
    }
}
