use p4_agent_core::agent::{Agent, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox::{self, Subscriptions};
use p4_protocol::frame::{self, Frame};
use p4_protocol::{Address, Envelope, QueueClass, Recipient};
use p4_service::message::ToAgent;
use p4_service::message::wire::encode_to_agent;
use p4_service::{Bodies, Registry, Standard};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn capability() -> String {
    "outer~".to_owned() + &"ab".repeat(32)
}

fn envelope(address: &Address, channel: &str, lane: QueueClass) -> Envelope {
    Envelope {
        target: address.clone(),
        recipient: Recipient::Agent,
        lane,
        route: "ack-e2e".into(),
        request_id: "ack-e2e".into(),
        stream_id: "stream".into(),
        origin_agent: Some(address.clone()),
        return_channel: Some(channel.to_owned()),
        ingress_generation: 0,
        event_seq: 0,
        deadline_unix_ms: 0,
        reply_to: Some(address.clone()),
        chain: None,
    }
}

fn backend_registry() -> Registry {
    Registry::new()
}

async fn read_frame(stream: &mut TcpStream) -> Frame {
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).await.unwrap();
    let total = frame::frame_len(&header).unwrap();
    let mut bytes = Vec::with_capacity(total);
    bytes.extend_from_slice(&header);
    bytes.resize(total, 0);
    stream.read_exact(&mut bytes[16..]).await.unwrap();
    frame::decode(&bytes).unwrap()
}

async fn send_ack(
    stream: &mut TcpStream,
    address: &Address,
    envelope_channel: &str,
    body_channel: &str,
    event_seq: u64,
) {
    let envelope = envelope(address, envelope_channel, QueueClass::Control);
    stream
        .write_all(
            &frame::encode(
                &envelope,
                &encode_to_agent(&ToAgent::Acknowledge {
                    return_channel: body_channel.to_owned(),
                    stream_id: "stream".into(),
                    event_seq,
                }),
            )
            .unwrap(),
        )
        .await
        .unwrap();
}

#[test]
fn tcp_stale_and_mismatched_acks_are_rejected_before_current_ack_succeeds() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let address = Address::tcp("127.0.0.1", port);
        let channel = capability();
        let subscriptions = Subscriptions::default();
        let (agent, receiver, in_flight) = Agent::new_with_subscriptions(
            address.clone(),
            Arc::new(Standard::new(backend_registry())),
            Arc::new(Bodies::default()),
            Lanes::default(),
            Budget::default(),
            subscriptions.clone(),
        );
        tokio::spawn(inbox::serve_with_subscriptions(
            listener,
            agent.queue(),
            16,
            subscriptions.clone(),
        ));
        tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let bind = envelope(&address, &channel, QueueClass::Control);
        client
            .write_all(
                &frame::encode(
                    &bind,
                    &encode_to_agent(&ToAgent::Acknowledge {
                        return_channel: channel.clone(),
                        stream_id: "stream".into(),
                        event_seq: 0,
                    }),
                )
                .unwrap(),
            )
            .await
            .unwrap();

        let mut event = Frame {
            envelope: envelope(&address, &channel, QueueClass::Response),
            body: Vec::new(),
        };
        event.envelope.event_seq = 7;
        let mut delivered = false;
        for _ in 0..100 {
            if agent.deliver_subscription(&channel, event.clone()).await {
                delivered = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(delivered);
        let delivered = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut client))
            .await
            .unwrap();
        assert_eq!(delivered.envelope.event_seq, 7);
        for _ in 0..100 {
            if agent.subscription_metrics().await.unacked == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(agent.subscription_metrics().await.unacked, 1);

        let mut rebound = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let bind = envelope(&address, &channel, QueueClass::Control);
        rebound
            .write_all(
                &frame::encode(
                    &bind,
                    &encode_to_agent(&ToAgent::Acknowledge {
                        return_channel: channel.clone(),
                        stream_id: "stream".into(),
                        event_seq: 0,
                    }),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let replay = tokio::time::timeout(Duration::from_secs(1), read_frame(&mut rebound))
            .await
            .unwrap();
        assert_eq!(replay.envelope.event_seq, 7);

        // The first socket retains its old ingress generation. Its ACK must
        // reach Duties and still fail the current subscription generation.
        send_ack(&mut client, &address, &channel, &channel, 7).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(agent.subscription_metrics().await.unacked, 1);
        // Both bind frames carried an event_seq=0 ACK and were rejected before
        // this stale ACK, so the aggregate is already three.
        assert_eq!(agent.ack_rejected(), 3);

        // Even on the current socket, a body naming another channel cannot
        // erase this channel's journal.
        let other_channel = "other~".to_owned() + &"cd".repeat(32);
        send_ack(&mut rebound, &address, &channel, &other_channel, 7).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(agent.subscription_metrics().await.unacked, 1);
        assert_eq!(agent.ack_rejected(), 4);

        send_ack(&mut rebound, &address, &channel, &channel, 7).await;

        for _ in 0..100 {
            if agent.subscription_metrics().await.unacked == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(agent.subscription_metrics().await.unacked, 0);
        assert_eq!(agent.ack_rejected(), 4);
    });
}
