use sha2::Sha256;

pub const CKPT_INTERVAL: usize = 100;
pub const MAX_RETRY: usize = 10;
pub type DefaultDigest = Sha256;
