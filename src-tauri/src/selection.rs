//! A captured selection belongs to exactly one request and one source window.

#[derive(Clone)]
pub struct Selection {
    pub request_id: u64,
    pub text: String,
    pub source: usize,
    /// `false` when the text could only be copied, so it is explained instead of replaced.
    pub is_editable: bool,
}

#[derive(Default)]
pub struct PendingSelection(Option<Selection>);

impl PendingSelection {
    pub fn current(&self) -> Option<&Selection> {
        self.0.as_ref()
    }

    /// A copied selection has nothing to restore, so a new capture may discard it.
    pub fn discard_read_only(&mut self) {
        if self.0.as_ref().is_some_and(|s| !s.is_editable) {
            self.0 = None;
        }
    }

    pub fn begin(&mut self, selection: Selection) {
        // Never overwrite an original that has not been restored or replaced.
        assert!(self.0.is_none());
        self.0 = Some(selection);
    }

    pub fn get(&self, request_id: u64) -> Option<&Selection> {
        self.current().filter(|s| s.request_id == request_id)
    }

    pub fn take(&mut self, request_id: u64) -> Option<Selection> {
        self.get(request_id)?;
        self.0.take()
    }

    pub fn retry(&mut self, request_id: u64, next_id: u64) -> Option<Selection> {
        self.get(request_id)?;
        let selection = self.0.as_mut()?;
        selection.request_id = next_id;
        Some(selection.clone())
    }
}

pub fn focus_matches(source: usize, foreground: usize) -> bool {
    source != 0 && foreground == source
}

#[derive(Debug, PartialEq)]
pub struct Captured {
    pub text: String,
    pub is_editable: bool,
}

/// A failed clear, cut or copy must never fall through to reading old clipboard data.
/// Text that cannot be cut (e.g. on a web page) falls back to a copy.
pub fn capture(
    clear: impl FnOnce() -> Result<(), String>,
    cut: impl FnOnce() -> Result<(), String>,
    copy: impl FnOnce() -> Result<(), String>,
    mut read: impl FnMut() -> Result<String, String>,
) -> Result<Captured, String> {
    clear()?;
    cut()?;
    let text = read()?;
    if !text.is_empty() {
        return Ok(Captured {
            text,
            is_editable: true,
        });
    }
    copy()?;
    Ok(Captured {
        text: read()?,
        is_editable: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(id: u64, source: usize) -> Selection {
        Selection {
            request_id: id,
            source,
            text: "private original".into(),
            is_editable: true,
        }
    }

    #[test]
    fn completed_selection_cannot_be_restored_into_next_window() {
        let mut pending = PendingSelection::default();
        pending.begin(selection(1, 100));
        assert_eq!(pending.take(1).unwrap().source, 100);
        // A failed capture in another window does not create a selection.
        assert!(pending.take(1).is_none());
        assert!(pending.take(2).is_none());
        pending.begin(selection(3, 200));
        assert!(pending.take(1).is_none());
        assert_eq!(pending.take(3).unwrap().source, 200);
        assert!(pending.current().is_none());
    }

    #[test]
    fn retry_keeps_original_target_and_rejects_old_commands() {
        let mut pending = PendingSelection::default();
        pending.begin(selection(1, 100));
        let retried = pending.retry(1, 2).unwrap();
        assert_eq!(retried.text, "private original");
        assert_eq!(retried.source, 100);
        assert!(pending.take(1).is_none());
        assert!(pending.retry(1, 3).is_none());
        assert_eq!(pending.take(2).unwrap().source, 100);
    }

    #[test]
    fn new_capture_discards_only_read_only_selection() {
        let mut pending = PendingSelection::default();
        pending.begin(selection(1, 100));
        pending.discard_read_only();
        assert!(pending.current().is_some());
        pending.take(1);
        pending.begin(Selection {
            is_editable: false,
            ..selection(2, 100)
        });
        pending.discard_read_only();
        assert!(pending.current().is_none());
    }

    #[test]
    fn failed_clipboard_clear_never_cuts_or_reads_stale_secret() {
        let result = capture(
            || Err("clipboard locked".into()),
            || panic!("must not cut"),
            || panic!("must not copy"),
            || panic!("must not read old secret"),
        );
        assert_eq!(result.unwrap_err(), "clipboard locked");
    }

    #[test]
    fn failed_cut_never_reads_clipboard() {
        assert!(capture(
            || Ok(()),
            || Err("cut failed".into()),
            || panic!("must not copy"),
            || panic!("must not read clipboard")
        )
        .is_err());
        assert!(capture(
            || Ok(()),
            || Ok(()),
            || panic!("must not copy"),
            || Err("read failed".into())
        )
        .is_err());
    }

    #[test]
    fn cut_text_is_editable_and_never_copied() {
        assert_eq!(
            capture(
                || Ok(()),
                || Ok(()),
                || panic!("must not copy"),
                || Ok("cut".into())
            )
            .unwrap(),
            Captured {
                text: "cut".into(),
                is_editable: true,
            }
        );
    }

    #[test]
    fn uncuttable_text_falls_back_to_copy() {
        let mut reads = vec![Ok("copied".into()), Ok(String::new())];
        assert_eq!(
            capture(|| Ok(()), || Ok(()), || Ok(()), || reads.pop().unwrap()).unwrap(),
            Captured {
                text: "copied".into(),
                is_editable: false,
            }
        );
        let mut reads = vec![Ok(String::new()), Ok(String::new())];
        assert_eq!(
            capture(|| Ok(()), || Ok(()), || Ok(()), || reads.pop().unwrap())
                .unwrap()
                .text,
            ""
        );
    }

    #[test]
    fn failed_copy_never_reads_clipboard_again() {
        let mut reads = 0;
        let result = capture(
            || Ok(()),
            || Ok(()),
            || Err("copy failed".into()),
            || {
                reads += 1;
                assert_eq!(reads, 1, "must not read after a failed copy");
                Ok(String::new())
            },
        );
        assert_eq!(result.unwrap_err(), "copy failed");
    }

    #[test]
    fn unknown_or_changed_foreground_is_not_confirmation() {
        assert!(!focus_matches(123, 0));
        assert!(!focus_matches(0, 0));
        assert!(!focus_matches(123, 456));
        assert!(focus_matches(123, 123));
    }
}
