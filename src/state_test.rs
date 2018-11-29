use crate::config::Config;
use crate::host::Host;
use crate::message::*;
use crate::state::PbftState;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

struct Payload {
    from: String,
    body: Vec<u8>,
}

struct Registry {
    nodes: HashMap<String, Mutex<Sender<Payload>>>,
}

struct NodeHost {
    id: String,
    registry: Arc<Registry>,
}

impl Host for NodeHost {
    fn send_message(&self, peer: &str, msg: &[u8]) {
        let sender = self
            .registry
            .nodes
            .get(peer)
            .unwrap()
            .lock()
            .unwrap()
            .clone();
        sender
            .send(Payload {
                from: self.id.clone(),
                body: msg.to_vec(),
            })
            .unwrap();
    }

    fn apply_commit(&mut self, content: &[u8]) {
        println!(
            "[{}] CONTENT = {}",
            self.id,
            ::std::str::from_utf8(content).unwrap()
        );
    }
}

#[test]
fn test_mock_network() {
    let node_ids: Vec<String> = vec![
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "d".to_string(),
    ];
    let mut senders: HashMap<String, Mutex<Sender<Payload>>> = HashMap::new();
    let mut receivers: HashMap<String, Option<Receiver<Payload>>> = HashMap::new();

    for id in &node_ids {
        let (tx, rx) = channel();
        senders.insert(id.clone(), Mutex::new(tx));
        receivers.insert(id.clone(), Some(rx));
    }

    let registry = Arc::new(Registry { nodes: senders });

    for id in &node_ids {
        let mut state = PbftState::new(
            Config {
                id: id.clone(),
                nodes: node_ids.clone(),
            },
            NodeHost {
                id: id.clone(),
                registry: registry.clone(),
            },
        );
        let receiver = receivers.get_mut(id).unwrap().take().unwrap();
        spawn(move || loop {
            let payload = receiver.recv().unwrap();
            state.input_message(payload.from, &payload.body);
        });
    }

    let sender = registry
        .nodes
        .get(&node_ids[1]) // test message forwarding
        .unwrap()
        .lock()
        .unwrap()
        .clone();
    sender
        .send(Payload {
            from: "".into(),
            body: ::bincode::serialize(&Message::Init(InitMessage {
                content: "hello world".to_string().into_bytes(),
            }))
            .unwrap(),
        })
        .unwrap();

    ::std::thread::sleep(::std::time::Duration::from_millis(1000));
    panic!();
}
