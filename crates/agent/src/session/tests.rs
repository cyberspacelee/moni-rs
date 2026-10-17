use clap::Parser;
use moni_protocol::MinuteRollup;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    net::TcpListener,
};
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, frame::coding::CloseCode};

use super::*;
use crate::window::sample;

fn config(server: &str) -> Config {
    Config::try_parse_from([
        "agent",
        "--server",
        server,
        "--token",
        "test-token",
        "--interval",
        "5",
    ])
    .unwrap()
}

fn host() -> HostInfo {
    HostInfo {
        hostname: "test".into(),
        os: "Linux".into(),
        kernel: "test".into(),
        arch: "x86_64".into(),
        cpu_model: "test CPU".into(),
        cpu_cores: 2,
        boot_time_ms: 1_000,
    }
}

fn minute(minute_ms: i64) -> MinuteRollup {
    MinuteRollup {
        minute_ms,
        samples: 60,
        ..MinuteRollup::default()
    }
}

async fn pair() -> (WebSocketStream<DuplexStream>, WebSocketStream<DuplexStream>) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    tokio::join!(
        WebSocketStream::from_raw_socket(client, Role::Client, None),
        WebSocketStream::from_raw_socket(server, Role::Server, None),
    )
}

async fn next_agent<S>(socket: &mut WebSocketStream<S>) -> AgentMessage
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = socket.next().await.unwrap().unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

async fn hub<S>(socket: &mut WebSocketStream<S>, message: HubMessage)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    socket
        .send(Message::text(serde_json::to_string(&message).unwrap()))
        .await
        .unwrap();
}

async fn welcome<S>(socket: &mut WebSocketStream<S>, report_interval_s: u32)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    assert!(matches!(
        next_agent(socket).await,
        AgentMessage::Hello {
            protocol: PROTOCOL_VERSION,
            ..
        }
    ));
    hub(
        socket,
        HubMessage::Welcome {
            server_id: "test".into(),
            report_interval_s,
        },
    )
    .await;
}

async fn close<S>(socket: &mut WebSocketStream<S>, code: u16)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    socket
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: "test".into(),
        })))
        .await
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn hello_waits_for_welcome_and_uses_the_hub_interval() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    state.window.borrow_mut().push(&sample(0, 90.0, 0, 0, 1));
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        assert!(matches!(
            next_agent(&mut server).await,
            AgentMessage::Hello { .. }
        ));
        assert!(
            timeout(Duration::from_secs(3), server.next())
                .await
                .is_err()
        );
        hub(
            &mut server,
            HubMessage::Welcome {
                server_id: "test".into(),
                report_interval_s: 2,
            },
        )
        .await;
        tokio::task::yield_now().await;
        state
            .window
            .borrow_mut()
            .push(&sample(4_000, 20.0, 100, 200, 1));
        let started = Instant::now();
        let AgentMessage::Metrics(frame) = next_agent(&mut server).await else {
            panic!("expected Metrics");
        };
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        assert_eq!(frame.cpu_pct, 20.0);
        assert_eq!((frame.net_rx_bps, frame.net_tx_bps), (100, 200));
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
}

#[tokio::test(start_paused = true)]
async fn welcome_timeout_is_ten_seconds() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        next_agent(&mut server).await;
        tokio::time::sleep(Duration::from_secs(11)).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Retry("Welcome timed out after 10 seconds")));
}

#[tokio::test(start_paused = true)]
async fn reconnect_resends_in_order_and_ack_removes_only_its_minute() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    state.queue.borrow_mut().insert(minute(60_000));
    state.queue.borrow_mut().insert(minute(0));
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        welcome(&mut server, 5).await;
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(0))
        );
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(60_000))
        );
        hub(&mut server, HubMessage::Ack { minute_ms: 0 }).await;
        server.send(Message::Ping("barrier".into())).await.unwrap();
        assert_eq!(
            server.next().await.unwrap().unwrap(),
            Message::Pong("barrier".into())
        );
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));

    let (mut client, mut server) = pair().await;
    let peer = async {
        welcome(&mut server, 5).await;
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(60_000))
        );
        state.queue.borrow_mut().insert(minute(120_000));
        state.ready.notify_one();
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(120_000))
        );
        assert!(
            timeout(Duration::from_secs(1), server.next())
                .await
                .is_err()
        );
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
}

#[tokio::test(start_paused = true)]
async fn sends_protocol_ping_after_thirty_seconds() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        welcome(&mut server, 5).await;
        let started = Instant::now();
        assert!(matches!(
            server.next().await.unwrap().unwrap(),
            Message::Ping(_)
        ));
        assert_eq!(started.elapsed(), Duration::from_secs(30));
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
}

#[tokio::test(start_paused = true)]
async fn fatal_close_codes_are_preserved_before_welcome() {
    for code in [4001, 4002, 4003] {
        let config = config("ws://localhost/ws/agent");
        let state = State::new(&config);
        let host = host();
        let (mut client, mut server) = pair().await;
        let peer = async {
            next_agent(&mut server).await;
            close(&mut server, code).await;
        };
        let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
        assert_eq!(end, Err(End::Closed(Some(code))));
    }
}

#[tokio::test]
async fn http_401_before_upgrade_is_a_fatal_token_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config(&format!("ws://{}/ws/agent", listener.local_addr().unwrap()));
    let state = State::new(&config);
    let host = host();
    let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let peer = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 8192);
            request.push(stream.read_u8().await.unwrap());
        }
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    };
    let (end, ()) = timeout(Duration::from_secs(5), async {
        tokio::join!(run(&config, &host, &state, &mut shutdown_rx), peer)
    })
    .await
    .unwrap();
    assert_eq!(end, End::Closed(Some(4003)));
}

#[tokio::test]
async fn shutdown_sends_a_close_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = config(&format!("ws://{}/ws/agent", listener.local_addr().unwrap()));
    let state = State::new(&config);
    let host = host();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let peer = async {
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();
        welcome(&mut server, 5).await;
        shutdown_tx.send(true).unwrap();
        assert!(matches!(
            server.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        server.flush().await.unwrap();
    };
    let (end, ()) = timeout(Duration::from_secs(5), async {
        tokio::join!(run(&config, &host, &state, &mut shutdown_rx), peer)
    })
    .await
    .unwrap();
    assert_eq!(end, End::Shutdown);
}
