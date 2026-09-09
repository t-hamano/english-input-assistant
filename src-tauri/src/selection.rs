//! A cut selection belongs to exactly one request and one source window.

#[derive(Clone)]
pub struct Selection {
    pub request_id: u64,
    pub text: String,
    pub source: usize,
}

#[derive(Default)]
pub struct PendingSelection(Option<Selection>);

impl PendingSelection {
    pub fn current(&self) -> Option<&Selection> {
        self.0.as_ref()
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

/// A failed clear or cut must never fall through to reading old clipboard data.
pub fn capture(
    clear: impl FnOnce() -> Result<(), String>,
    cut: impl FnOnce() -> Result<(), String>,
    read: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    clear()?;
    cut()?;
    read()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(id: u64, source: usize) -> Selection {
        Selection { request_id: id, source, text: "private original".into() }
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
    fn failed_clipboard_clear_never_cuts_or_reads_stale_secret() {
        let result = capture(
            || Err("clipboard locked".into()),
            || panic!("must not cut"),
            || panic!("must not read old secret"),
        );
        assert_eq!(result.unwrap_err(), "clipboard locked");
    }

    #[test]
    fn failed_cut_never_reads_clipboard() {
        assert!(capture(|| Ok(()), || Err("cut failed".into()),
            || panic!("must not read clipboard")).is_err());
        assert!(capture(|| Ok(()), || Ok(()), || Err("read failed".into())).is_err());
        assert_eq!(capture(|| Ok(()), || Ok(()), || Ok(String::new())).unwrap(), "");
    }

    #[test]
    fn unknown_or_changed_foreground_is_not_confirmation() {
        assert!(!focus_matches(123, 0));
        assert!(!focus_matches(0, 0));
        assert!(!focus_matches(123, 456));
        assert!(focus_matches(123, 123));
    }
}
