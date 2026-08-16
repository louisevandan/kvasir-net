use super::*;
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::channel;
use p4_protocol::{Address, Envelope, QueueClass, Recipient};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

fn frame(route: &str, lane: QueueClass) -> Frame {
    Frame {
        envelope: Envelope {
            target: Address::tcp("127.0.0.1", 19001),
            recipient: Recipient::Agent,
            lane,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: b"body".to_vec(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn what_arrives_on_a_socket_lands_on_the_queue_unchanged() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let sent = frame("r1", QueueClass::Prefill);
        let bytes = frame::encode(&sent.envelope, &sent.body).unwrap();
        client.write_all(&bytes).await.unwrap();

        let arrived = tokio::time::timeout(Duration::from_secs(2), receiver.take())
            .await
            .expect("a frame arrived")
            .expect("the queue is open");
        assert_eq!(arrived, sent);
    });
}

#[test]
fn several_frames_on_one_connection_all_arrive() {
    // A connection is multiplexed: many routes share it, and the reader must
    // never be the thing that serialises them.
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut bytes = Vec::new();
        for index in 0..8 {
            let one = frame(&format!("r{index}"), QueueClass::Control);
            bytes.extend_from_slice(&frame::encode(&one.envelope, &one.body).unwrap());
        }
        client.write_all(&bytes).await.unwrap();

        for index in 0..8 {
            let arrived = tokio::time::timeout(Duration::from_secs(2), receiver.take())
                .await
                .expect("a frame arrived")
                .unwrap();
            assert_eq!(arrived.envelope.route, format!("r{index}"));
        }
    });
}

#[test]
fn a_frame_of_a_foreign_version_closes_the_connection_rather_than_being_read() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let one = frame("r1", QueueClass::Control);
        let mut bytes = frame::encode(&one.envelope, &one.body).unwrap();
        bytes[4] = 5;
        client.write_all(&bytes).await.unwrap();

        // Nothing reaches the queue, and the reader gives up on the connection
        // instead of trying to interpret the rest of the stream.
        let nothing = tokio::time::timeout(Duration::from_millis(200), receiver.take()).await;
        assert!(nothing.is_err());
    });
}
