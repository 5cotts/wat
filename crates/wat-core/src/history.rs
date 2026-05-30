/// A fixed-capacity ring buffer of recent shell commands.
pub struct History {
    entries: Vec<String>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        History {
            entries: Vec::new(),
            capacity,
        }
    }

    pub fn push(&mut self, cmd: impl Into<String>) {
        let cmd = cmd.into();
        if cmd.is_empty() {
            return;
        }
        // Avoid consecutive duplicates
        if self.entries.last().map(|s| s.as_str()) == Some(cmd.as_str()) {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.remove(0);
        }
        self.entries.push(cmd);
    }

    /// Return command by recency index: 0 = most recent, 1 = second most recent, …
    pub fn at(&self, index: usize) -> Option<&str> {
        let len = self.entries.len();
        if index >= len {
            return None;
        }
        Some(&self.entries[len - 1 - index])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate oldest→newest.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_at() {
        let mut h = History::new(10);
        h.push("a");
        h.push("b");
        h.push("c");
        assert_eq!(h.at(0), Some("c"));
        assert_eq!(h.at(1), Some("b"));
        assert_eq!(h.at(2), Some("a"));
        assert_eq!(h.at(3), None);
    }

    #[test]
    fn no_consecutive_duplicates() {
        let mut h = History::new(10);
        h.push("ls");
        h.push("ls");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn ring_capacity() {
        let mut h = History::new(3);
        h.push("a");
        h.push("b");
        h.push("c");
        h.push("d");
        assert_eq!(h.len(), 3);
        assert_eq!(h.at(0), Some("d"));
        assert_eq!(h.at(2), Some("b")); // "a" was evicted
    }

    #[test]
    fn empty_commands_not_stored() {
        let mut h = History::new(10);
        h.push("");
        assert_eq!(h.len(), 0);
    }
}
