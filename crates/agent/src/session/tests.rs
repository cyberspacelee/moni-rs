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

async fn set_interval<S>(socket: &mut WebSocketStream<S>, report_interval_s: u32)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    hub(socket, HubMessage::SetReportInterval { report_interval_s }).await;
    socket
        .send(Message::Ping("interval-applied".into()))
        .await
        .unwrap();
    assert_eq!(
        socket.next().await.unwrap().unwrap(),
        Message::Pong("interval-applied".into())
    );
}

#[tokio::test(start_paused = true)]
async fn updates_without_local_permission_or_secure_transport_fail_without_interrupting_metrics() {
    for (server_url, repo, expected_error) in [
        ("wss://example.com/ws/agent", None, UpdateError::Disabled),
        (
            "ws://localhost/ws/agent",
            Some("owner/repo"),
            UpdateError::InsecureTransport,
        ),
    ] {
        let mut config = config(server_url);
        config.update_repo = repo.map(str::to_owned);
        let state = State::new(&config);
        let host = host();
        let (mut client, mut server) = pair().await;
        let directory = std::env::temp_dir().join(format!(
            "moni-agent-missing-update-directory-{}",
            std::process::id()
        ));
        let peer = async {
            assert!(matches!(
                next_agent(&mut server).await,
                AgentMessage::Hello {
                    update_repo: None,
                    ..
                }
            ));
            hub(
                &mut server,
                HubMessage::Welcome {
                    server_id: "test".into(),
                    report_interval_s: 2,
                },
            )
            .await;
            hub(
                &mut server,
                HubMessage::Update {
                    job_id: "0123456789abcdef0123456789abcdef".into(),
                    version: "v999.0.0".into(),
                    repo: "owner/repo".into(),
                },
            )
            .await;
            assert!(
                matches!(next_agent(&mut server).await, AgentMessage::UpdateStatus { state: moni_protocol::UpdateState::Failed, error: Some(error), .. } if error == expected_error)
            );
            state
                .window
                .borrow_mut()
                .push(&sample(1_000, 20.0, 100, 200, 1));
            assert!(matches!(
                next_agent(&mut server).await,
                AgentMessage::Metrics(_)
            ));
            close(&mut server, 4000).await;
        };
        let (end, ()) = tokio::join!(
            communicate_at(&mut client, &config, &host, &state, &directory),
            peer
        );
        assert_eq!(end, Err(End::Closed(Some(4000))));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn waiting_for_update_io_keeps_metrics_rollups_acks_pings_and_interval_commands_running() {
    use std::os::unix::fs::DirBuilderExt;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (release, blocked) = std::sync::mpsc::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let blocker = tokio::task::spawn_blocking(move || { let _ = started.send(()); let _ = blocked.recv_timeout(Duration::from_secs(5)); });
        ready.await.unwrap();
        tokio::time::pause();

        let directory = std::env::temp_dir().join(format!("moni-agent-session-update-{}-{}", std::process::id(), crate::now_ms()));
        std::fs::DirBuilder::new().mode(0o700).create(&directory).unwrap();
        let mut config = config("wss://example.com/ws/agent");
        config.update_repo = Some("owner/repo".into());
        let state = State::new(&config);
        let host = host();
        let (mut client, mut server) = pair().await;
        let job = "0123456789abcdef0123456789abcdef";
        let peer = async {
            assert!(matches!(next_agent(&mut server).await, AgentMessage::Hello { update_repo: Some(repo), .. } if repo == "owner/repo"));
            hub(&mut server, HubMessage::Welcome { server_id: "test".into(), report_interval_s: 2 }).await;
            let command = HubMessage::Update { job_id: job.into(), version: "v999.0.0".into(), repo: "owner/repo".into() };
            hub(&mut server, command.clone()).await;
            server.send(Message::Ping("update-queued".into())).await.unwrap();
            assert_eq!(server.next().await.unwrap().unwrap(), Message::Pong("update-queued".into()));
            assert!(!directory.join("update-request").exists());
            hub(&mut server, command.clone()).await;
            hub(&mut server, HubMessage::Update { job_id: "fedcba9876543210fedcba9876543210".into(), version: "v1000.0.0".into(), repo: "owner/repo".into() }).await;
            assert!(matches!(next_agent(&mut server).await, AgentMessage::UpdateStatus { state: moni_protocol::UpdateState::Failed, error: Some(UpdateError::Busy), .. }));

            state.queue.borrow_mut().insert(minute(60_000));
            state.ready.notify_one();
            assert_eq!(next_agent(&mut server).await, AgentMessage::Rollup(minute(60_000)));
            hub(&mut server, HubMessage::Ack { minute_ms: 60_000 }).await;
            set_interval(&mut server, 30).await;
            state.window.borrow_mut().push(&sample(1_000, 90.0, 9_000, 18_000, 1));
            set_interval(&mut server, 2).await;
            state.window.borrow_mut().push(&sample(2_000, 20.0, 100, 200, 1));
            state.sample_ready.notify_one();
            let AgentMessage::Metrics(frame) = next_agent(&mut server).await else { panic!("expected Metrics"); };
            assert_eq!(frame.cpu_pct, 20.0);
            tokio::time::advance(Duration::from_secs(31)).await;
            assert!(matches!(server.next().await.unwrap().unwrap(), Message::Ping(_)));
            server.flush().await.unwrap();

            release.send(()).unwrap();
            blocker.await.unwrap();
            assert!(matches!(next_agent(&mut server).await, AgentMessage::UpdateStatus { job_id, version, state: moni_protocol::UpdateState::Accepted, error: None } if job_id == job && version == "v999.0.0"));
            assert_eq!(std::fs::read_to_string(directory.join("update-request")).unwrap(), "v999.0.0\n");
            assert_eq!(std::fs::read_to_string(directory.join("health")).unwrap(), format!("{}\n{}\n", env!("CARGO_PKG_VERSION"), std::process::id()));
            hub(&mut server, command).await;
            assert!(matches!(next_agent(&mut server).await, AgentMessage::UpdateStatus { state: moni_protocol::UpdateState::Accepted, error: None, .. }));
            close(&mut server, 4000).await;
        };
        let (end, ()) = tokio::join!(communicate_at(&mut client, &config, &host, &state, &directory), peer);
        assert_eq!(end, Err(End::Closed(Some(4000))));
        state.queue.borrow_mut().resend_all();
        assert!(state.queue.borrow_mut().take_unsent().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    });
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
async fn dynamic_intervals_slow_reports_and_send_a_fresh_sample_immediately_when_faster() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        welcome(&mut server, 2).await;
        set_interval(&mut server, 15).await;
        state
            .window
            .borrow_mut()
            .push(&sample(1_000, 20.0, 100, 200, 1));
        let started = Instant::now();
        assert!(matches!(
            next_agent(&mut server).await,
            AgentMessage::Metrics(_)
        ));
        assert_eq!(started.elapsed(), Duration::from_secs(15));

        state
            .window
            .borrow_mut()
            .push(&sample(15_000, 90.0, 9_000, 18_000, 5));
        state.sample_ready.notify_one();
        set_interval(&mut server, 2).await;
        assert!(
            timeout(Duration::from_millis(500), server.next())
                .await
                .is_err()
        );
        state
            .window
            .borrow_mut()
            .push(&sample(16_000, 10.0, 100, 200, 1));
        state.sample_ready.notify_one();
        let started = Instant::now();
        let AgentMessage::Metrics(frame) = next_agent(&mut server).await else {
            panic!("expected Metrics");
        };
        assert_eq!(started.elapsed(), Duration::ZERO);
        assert_eq!(frame.cpu_pct, 10.0);
        assert_eq!((frame.net_rx_bps, frame.net_tx_bps), (100, 200));

        state
            .window
            .borrow_mut()
            .push(&sample(17_000, 30.0, 300, 600, 1));
        state.sample_ready.notify_one();
        let started = Instant::now();
        tokio::time::sleep(Duration::from_secs(1)).await;
        set_interval(&mut server, 2).await;
        assert!(matches!(
            next_agent(&mut server).await,
            AgentMessage::Metrics(_)
        ));
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
}

#[tokio::test(start_paused = true)]
async fn dynamic_intervals_keep_rollups_acks_and_the_ping_schedule() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    state.queue.borrow_mut().insert(minute(0));
    let host = host();
    let (mut client, mut server) = pair().await;
    let peer = async {
        welcome(&mut server, 2).await;
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(0))
        );
        hub(&mut server, HubMessage::Ack { minute_ms: 0 }).await;
        let started = Instant::now();
        set_interval(&mut server, 15).await;
        state.queue.borrow_mut().insert(minute(60_000));
        state.ready.notify_one();
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(60_000))
        );
        hub(&mut server, HubMessage::Ack { minute_ms: 60_000 }).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        set_interval(&mut server, 2).await;
        assert!(matches!(
            server.next().await.unwrap().unwrap(),
            Message::Ping(_)
        ));
        assert_eq!(started.elapsed(), Duration::from_secs(30));
        server.flush().await.unwrap();
        state.queue.borrow_mut().insert(minute(120_000));
        state.ready.notify_one();
        assert_eq!(
            next_agent(&mut server).await,
            AgentMessage::Rollup(minute(120_000))
        );
        hub(&mut server, HubMessage::Ack { minute_ms: 120_000 }).await;
        set_interval(&mut server, 15).await;
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
    state.queue.borrow_mut().resend_all();
    assert!(state.queue.borrow_mut().take_unsent().is_empty());
}

#[tokio::test(start_paused = true)]
async fn welcome_and_interval_changes_reject_zero() {
    for initial in [true, false] {
        let config = config("ws://localhost/ws/agent");
        let state = State::new(&config);
        let host = host();
        let (mut client, mut server) = pair().await;
        let peer = async {
            welcome(&mut server, if initial { 0 } else { 15 }).await;
            if !initial {
                hub(
                    &mut server,
                    HubMessage::SetReportInterval {
                        report_interval_s: 0,
                    },
                )
                .await;
            }
        };
        let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
        assert_eq!(
            end,
            Err(End::Retry(if initial {
                "expected Welcome with a positive report interval"
            } else {
                "expected a positive report interval"
            }))
        );
    }
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

#[tokio::test(start_paused = true)]
async fn unacknowledged_rollup_is_retried_on_a_live_connection() {
    let config = config("ws://localhost/ws/agent");
    let state = State::new(&config);
    state.queue.borrow_mut().insert(minute(0));
    state.queue.borrow_mut().insert(minute(60_000));
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
        hub(&mut server, HubMessage::Ack { minute_ms: 60_000 }).await;
        let started = Instant::now();
        loop {
            match server.next().await.unwrap().unwrap() {
                Message::Ping(_) => {
                    server.flush().await.unwrap();
                }
                Message::Text(text) => {
                    assert_eq!(
                        serde_json::from_str::<AgentMessage>(&text).unwrap(),
                        AgentMessage::Rollup(minute(0))
                    );
                    assert_eq!(started.elapsed(), Duration::from_secs(90));
                    break;
                }
                other => panic!("Unexpected frame: {other:?}"),
            }
        }
        hub(&mut server, HubMessage::Ack { minute_ms: 0 }).await;
        close(&mut server, 4000).await;
    };
    let (end, ()) = tokio::join!(communicate(&mut client, &config, &host, &state), peer);
    assert_eq!(end, Err(End::Closed(Some(4000))));
    assert!(state.queue.borrow_mut().take_unsent().is_empty());
}
