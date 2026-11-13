/// Fixed-capacity stack of held MIDI keys for last-note priority.
/// No allocation, so it's safe to use on the audio thread.
pub struct NoteStack {
    keys: [u8; Self::CAPACITY],
    len: usize,
}

impl Default for NoteStack {
    fn default() -> Self {
        Self {
            keys: [0; Self::CAPACITY],
            len: 0,
        }
    }
}

impl NoteStack {
    const CAPACITY: usize = 16;

    /// Push a key to the top. Re-pressing a held key moves it to the top.
    /// When full, the oldest key is dropped.
    pub fn push(&mut self, key: u8) {
        self.remove(key);
        if self.len == Self::CAPACITY {
            self.keys.copy_within(1.., 0);
            self.len -= 1;
        }
        self.keys[self.len] = key;
        self.len += 1;
    }

    pub fn remove(&mut self, key: u8) {
        if let Some(i) = self.keys[..self.len].iter().position(|&k| k == key) {
            self.keys.copy_within(i + 1..self.len, i);
            self.len -= 1;
        }
    }

    pub fn top(&self) -> Option<u8> {
        self.len.checked_sub(1).map(|i| self.keys[i])
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::NoteStack;

    #[test]
    fn last_note_priority() {
        let mut s = NoteStack::default();
        s.push(60);
        s.push(64);
        assert_eq!(s.top(), Some(64));
        s.remove(64);
        assert_eq!(s.top(), Some(60));
        s.remove(60);
        assert_eq!(s.top(), None);
    }

    #[test]
    fn repress_moves_to_top_and_overflow_drops_oldest() {
        let mut s = NoteStack::default();
        s.push(1);
        s.push(2);
        s.push(1);
        assert_eq!(s.top(), Some(1));
        s.remove(1);
        assert_eq!(s.top(), Some(2));

        let mut s = NoteStack::default();
        for k in 0..20 {
            s.push(k);
        }
        assert_eq!(s.top(), Some(19));
        for k in (4..20).rev() {
            s.remove(k);
        }
        assert_eq!(s.top(), None); // keys 0..4 were dropped on overflow
    }
}
