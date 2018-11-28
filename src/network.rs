pub trait MessageSender: Send {
    /// Reliably send a message to the given peer.
    fn send_message(&self, peer: &str, msg: &[u8]);
}
