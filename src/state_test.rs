use crate::config::Config;
use crate::host::Host;
use crate::message::*;
use crate::state::PbftState;
use rand::{thread_rng, RngCore};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::spawn;

#[derive(Clone, Default)]
struct MessageSink {
    messages: Arc<Mutex<HashMap<Vec<u8>, HashSet<String>>>>,
    orders: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,
}

impl MessageSink {
    fn put_message(&self, node_id: String, message: Vec<u8>) {
        self.messages
            .lock()
            .unwrap()
            .entry(message.clone())
            .or_insert(HashSet::new())
            .insert(node_id.clone());

        self.orders
            .lock()
            .unwrap()
            .entry(node_id)
            .or_insert(Vec::new())
            .push(message);
    }

    fn check(&self, expected_messages: Option<Vec<Vec<u8>>>, num_nodes: usize) {
        let messages = self.messages.lock().unwrap();

        if let Some(expected_messages) = expected_messages {
            for msg in &expected_messages {
                if let Some(v) = messages.get(msg) {
                    if v.len() != num_nodes {
                        panic!(
                            "v.len() != num_nodes: {} != {}, message = {:?}",
                            v.len(),
                            num_nodes,
                            msg
                        );
                    }
                } else {
                    panic!("expected message not found: {:?}", msg);
                }
            }
            println!(
                "verified {}/{} messages",
                expected_messages.len(),
                messages.len()
            );
        } else {
            for (k, v) in &*messages {
                if v.len() != num_nodes {
                    panic!(
                        "v.len() != num_nodes: {} != {}, message = {:?}",
                        v.len(),
                        num_nodes,
                        k
                    );
                }
            }
            println!("verified {} messages", messages.len());
        }

        let orders = self.orders.lock().unwrap();
        orders
            .iter()
            .fold(None, |last: Option<&Vec<Vec<u8>>>, (_k, v)| {
                if let Some(last) = last {
                    if *last != *v {
                        panic!("value/order mismatch: {:?} {:?}", last, v);
                    }
                }

                Some(v)
            });
    }
}

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
    message_sink: MessageSink,
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
        //println!("[{}] CONTENT = {:?}", self.id, content);
        self.message_sink
            .put_message(self.id.clone(), content.to_vec());
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
    let message_sink = MessageSink::default();

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
                message_sink: message_sink.clone(),
            },
        );
        let receiver = receivers.get_mut(id).unwrap().take().unwrap();
        spawn(move || loop {
            let payload = receiver.recv().unwrap();
            state.input_message(payload.from, &payload.body);
        });
    }

    let mut rng = thread_rng();

    let messages: Vec<Vec<u8>> = (0..100)
        .map(|_| {
            let mut v = vec![0; 32];
            rng.fill_bytes(&mut v);
            v
        })
        .collect();

    for msg in &messages {
        let msg = msg.clone();
        let sender = registry
            .nodes
            .get(&node_ids[(rng.next_u64() % node_ids.len() as u64) as usize])
            .unwrap()
            .lock()
            .unwrap()
            .clone();

        spawn(move || {
            sender
                .send(Payload {
                    from: "".into(),
                    body: ::bincode::serialize(&Message::Init(InitMessage { content: msg }))
                        .unwrap(),
                })
                .unwrap();
        });
    }

    ::std::thread::sleep(::std::time::Duration::from_millis(1000));
    message_sink.check(Some(messages), node_ids.len());
}
