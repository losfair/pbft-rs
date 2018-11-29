use bincode;
use crate::config::*;
use crate::host::Host;
use crate::message::*;
use crate::params::*;
use sha2::digest::FixedOutput;
use sha2::Digest;
use std::collections::HashSet;

pub struct PbftState {
    config: Config,
    local_index: usize,
    next_nonce: u64, // only used if the current node is primary
    host: Box<Host>,
    current_view: u64,
    log_ckpt_offset: u64,
    log_commit_offset: usize,
    logs: Vec<Option<LogEntry>>,
    retry_queue: Vec<Retry>,
}

struct Retry {
    remaining: usize,
    from: String,
    message: Message,
}

#[derive(Clone)]
struct LogEntry {
    view: u64,
    nonce: u64,
    content: Vec<u8>,
    digest: Vec<u8>,
    prepares: HashSet<String>, // a set of peers who have reported to be prepared
    commit_confirmed: HashSet<String>, // a set of peers who have reported to be committed
    pre_committed: bool,
    committed: bool,
}

impl PbftState {
    pub fn new<H: Host + Send + 'static>(config: Config, host: H) -> PbftState {
        let local_index = config
            .nodes
            .iter()
            .enumerate()
            .find(|(_, x)| **x == config.id)
            .unwrap()
            .0;

        PbftState {
            config: config,
            local_index: local_index,
            next_nonce: 0,
            host: Box::new(host),
            current_view: 0,
            log_ckpt_offset: 0,
            log_commit_offset: 0,
            logs: vec![None; CKPT_INTERVAL],
            retry_queue: Vec::new(),
        }
    }

    pub fn input_message(&mut self, from: String, msg: &[u8]) {
        let msg: Message = match bincode::deserialize(msg) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("{:?}", e);
                return;
            }
        };

        self.process_with_retry(Retry {
            remaining: MAX_RETRY,
            from: from,
            message: msg,
        });

        self.do_retry();
    }

    fn do_retry(&mut self) {
        let retry_queue = ::std::mem::replace(&mut self.retry_queue, Vec::new());
        for retry in retry_queue {
            self.process_with_retry(retry);
        }
    }

    fn process_with_retry(&mut self, retry: Retry) {
        let from = retry.from.clone();

        let success = match retry.message.clone() {
            Message::Init(msg) => self.handle_init(from, msg),
            Message::PrePrepare(msg) => self.handle_pre_prepare(from, msg),
            Message::Prepare(msg) => self.handle_prepare(from, msg),
            Message::Commit(msg) => self.handle_commit(from, msg),
        };

        if !success && retry.remaining > 0 {
            eprintln!("queueing for retry ({} time(s) remaining)", retry.remaining);
            self.retry_queue.push(Retry {
                remaining: retry.remaining - 1,
                from: retry.from,
                message: retry.message,
            });
        }
    }

    fn broadcast_message(&mut self, msg: &Message) {
        for peer in &self.config.nodes {
            self.host
                .send_message(peer, &bincode::serialize(msg).unwrap());
        }
    }

    fn do_commit(&mut self) {
        while self.log_commit_offset < self.logs.len() {
            if let Some(ref log) = self.logs[self.log_commit_offset] {
                if log.committed {
                    self.host.apply_commit(&log.content);
                    self.log_commit_offset += 1;
                    continue;
                }
            }

            break;
        }
    }

    fn handle_init(&mut self, _from: String, msg: InitMessage) -> bool {
        if self.is_primary() {
            let view = self.current_view;
            let nonce = self.next_nonce;
            self.next_nonce += 1;

            self.broadcast_message(&Message::PrePrepare(PrePrepareMessage {
                view: view,
                nonce: nonce,
                content: msg.content,
            }));
        } else {
            let primary = self.config.nodes[self.get_primary()].as_str();
            self.host
                .send_message(primary, &bincode::serialize(&Message::Init(msg)).unwrap());
        }

        true
    }

    fn get_primary(&self) -> usize {
        (self.current_view % (self.config.nodes.len() as u64)) as usize
    }

    fn is_primary(&self) -> bool {
        self.get_primary() == self.local_index
    }

    fn handle_pre_prepare(&mut self, from: String, msg: PrePrepareMessage) -> bool {
        if msg.view != self.current_view {
            eprintln!("view mismatch"); // TODO: what should we do here?
            return false;
        }

        if msg.nonce < self.log_ckpt_offset
            || msg.nonce - self.log_ckpt_offset >= CKPT_INTERVAL as u64
        {
            eprintln!("invalid nonce");
            return false;
        }

        if from != self.config.nodes[self.get_primary()] {
            eprintln!(
                "only primary node {} can send PrePrepapre, from = {}",
                self.config.nodes[self.get_primary()],
                from
            );
            return false;
        }

        let log_slot = (msg.nonce - self.log_ckpt_offset) as usize;
        if self.logs[log_slot].is_some() {
            eprintln!("duplicate log");
            return true;
        }

        let content_digest = DefaultDigest::default()
            .chain(&msg.content)
            .fixed_result()
            .as_slice()
            .to_vec();

        self.logs[log_slot] = Some(LogEntry {
            view: msg.view,
            nonce: msg.nonce,
            content: msg.content,
            digest: content_digest.clone(),
            prepares: HashSet::new(), // TODO: include self?
            commit_confirmed: HashSet::new(),
            pre_committed: false,
            committed: false,
        });

        self.broadcast_message(&Message::Prepare(PrepareMessage {
            view: msg.view,
            nonce: msg.nonce,
            digest: content_digest,
        }));

        true
    }

    fn handle_prepare(&mut self, from: String, msg: PrepareMessage) -> bool {
        if msg.view != self.current_view {
            eprintln!("view mismatch"); // TODO: what should we do here?
            return false;
        }

        if msg.nonce < self.log_ckpt_offset
            || msg.nonce - self.log_ckpt_offset >= CKPT_INTERVAL as u64
        {
            eprintln!("invalid nonce");
            return false;
        }

        let log_slot = (msg.nonce - self.log_ckpt_offset) as usize;

        if let Some(ref mut log) = self.logs[log_slot] {
            if log.digest != msg.digest {
                eprintln!("digest mismatch"); // TODO: what to do?
                return false;
            }

            log.prepares.insert(from);
            if !log.pre_committed && log.prepares.len() > self.config.nodes.len() * 2 / 3 {
                log.pre_committed = true;
                self.broadcast_message(&Message::Commit(CommitMessage {
                    view: msg.view,
                    nonce: msg.nonce,
                }));
            }

            true
        } else {
            eprintln!("log does not exist");
            false
        }
    }

    fn handle_commit(&mut self, from: String, msg: CommitMessage) -> bool {
        if msg.view != self.current_view {
            eprintln!("view mismatch"); // TODO: what should we do here?
            return false;
        }

        if msg.nonce < self.log_ckpt_offset
            || msg.nonce - self.log_ckpt_offset >= CKPT_INTERVAL as u64
        {
            eprintln!("invalid nonce");
            return false;
        }

        let log_slot = (msg.nonce - self.log_ckpt_offset) as usize;

        if let Some(ref mut log) = self.logs[log_slot] {
            log.commit_confirmed.insert(from);

            if log.pre_committed
                && !log.committed
                && log.commit_confirmed.len() > self.config.nodes.len() * 2 / 3
            {
                log.committed = true;
                self.do_commit();
            }
            true
        } else {
            eprintln!("log does not exist");
            false
        }
    }
}
