pub trait Host: Send {
    /// Reliably sends a message to the given peer.
    fn send_message(&self, peer: &str, msg: &[u8]);

    /// Applies a commit.
    fn apply_commit(&mut self, content: &[u8]);
}
