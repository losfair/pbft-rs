#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    Init(InitMessage),
    PrePrepare(PrePrepareMessage),
    Prepare(PrepareMessage),
    Commit(CommitMessage),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InitMessage {
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrePrepareMessage {
    pub view: u64,
    pub nonce: u64,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrepareMessage {
    pub view: u64,
    pub nonce: u64,
    pub digest: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitMessage {
    pub view: u64,
    pub nonce: u64,
}
