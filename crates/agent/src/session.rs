use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use moni_protocol::{AgentMessage, HostInfo, HubMessage, PROTOCOL_VERSION};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::watch,
    time::{Instant, MissedTickBehavior, interval_at, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        self, Message,
        client::IntoClientRequest,
        http::{HeaderValue, StatusCode},
    },
};
use tracing::info;

use crate::{State, config::Config};

const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Shutdown,
    Closed(Option<u16>),
    Retry(&'static str),
}

pub async fn run(
    config: &Config,
    host: &HostInfo,
    state: &State,
    shutdown: &mut watch::Receiver<bool>,
) -> End {
    let mut request = match config.server.as_str().into_client_request() {
        Ok(request) => request,
        Err(_) => return End::Retry("invalid WebSocket endpoint"),
    };
    let mut authorization = match HeaderValue::from_str(&format!("Bearer {}", config.token)) {
        Ok(value) => value,
        Err(_) => return End::Closed(Some(4003)),
    };
    authorization.set_sensitive(true);
    request.headers_mut().insert("authorization", authorization);
    let connected = tokio::select! {
        biased;
        _ = shutdown.changed() => return End::Shutdown,
        result = timeout(IO_TIMEOUT, tokio_tungstenite::connect_async(request)) => result,
    };
    let mut socket = match connected {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(tungstenite::Error::Http(response)))
            if response.status() == StatusCode::UNAUTHORIZED =>
        {
            return End::Closed(Some(4003));
        }
        Ok(Err(_)) => return End::Retry("WebSocket connection or HTTP upgrade failed"),
        Err(_) => return End::Retry("WebSocket connection timed out after 10 seconds"),
    };
    let end = tokio::select! {
        biased;
        _ = shutdown.changed() => End::Shutdown,
        result = communicate(&mut socket, config, host, state) => match result {
            Ok(()) => End::Closed(None),
            Err(end) => end,
        },
    };
    if end == End::Shutdown {
        let _ = timeout(Duration::from_secs(2), async {
            socket.close(None).await?;
            while let Some(message) = socket.next().await {
                if matches!(message?, Message::Close(_)) {
                    break;
                }
            }
            Ok::<_, tungstenite::Error>(())
        })
        .await;
    } else {
        let _ = timeout(Duration::from_secs(2), socket.flush()).await;
    }
    end
}

async fn send<S>(socket: &mut WebSocketStream<S>, message: Message) -> Result<(), End>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    match timeout(IO_TIMEOUT, socket.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(End::Retry("WebSocket send failed")),
        Err(_) => Err(End::Retry("WebSocket send timed out after 10 seconds")),
    }
}

async fn send_agent<S>(socket: &mut WebSocketStream<S>, message: &AgentMessage) -> Result<(), End>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let json =
        serde_json::to_string(message).map_err(|_| End::Retry("message serialization failed"))?;
    send(socket, Message::text(json)).await
}

async fn receive<S>(socket: &mut WebSocketStream<S>) -> Result<HubMessage, End>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|_| End::Retry("invalid Hub message"));
            }
            Some(Ok(Message::Close(frame))) => {
                return Err(End::Closed(frame.map(|frame| u16::from(frame.code))));
            }
            Some(Ok(Message::Ping(data))) => send(socket, Message::Pong(data)).await?,
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(_)) => return Err(End::Retry("expected a Hub text message")),
            Some(Err(_)) => return Err(End::Retry("WebSocket receive failed")),
            None => return Err(End::Closed(None)),
        }
    }
}

async fn send_pending<S>(socket: &mut WebSocketStream<S>, state: &State) -> Result<usize, End>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let pending = state.queue.borrow_mut().take_unsent();
    let count = pending.len();
    for rollup in pending {
        send_agent(socket, &AgentMessage::Rollup(rollup)).await?;
    }
    Ok(count)
}

async fn communicate<S>(
    socket: &mut WebSocketStream<S>,
    config: &Config,
    host: &HostInfo,
    state: &State,
) -> Result<(), End>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    send_agent(
        socket,
        &AgentMessage::Hello {
            protocol: PROTOCOL_VERSION,
            agent_version: env!("CARGO_PKG_VERSION").to_owned(),
            host: host.clone(),
        },
    )
    .await?;
    let welcome = timeout(IO_TIMEOUT, receive(socket))
        .await
        .map_err(|_| End::Retry("Welcome timed out after 10 seconds"))??;
    let report_interval_s = match welcome {
        HubMessage::Welcome {
            report_interval_s, ..
        } if report_interval_s > 0 => report_interval_s,
        _ => {
            return Err(End::Retry(
                "expected Welcome with a positive report interval",
            ));
        }
    };
    info!(
        report_interval_s,
        configured_interval_s = config.interval,
        "connected; Hello/Welcome complete"
    );
    *state.window.borrow_mut() = Default::default();
    state.queue.borrow_mut().resend_all();
    let count = send_pending(socket, state).await?;
    info!(count, "resent pending rollups");

    let now = Instant::now();
    let report_period = Duration::from_secs(u64::from(report_interval_s));
    let mut report = interval_at(now + report_period, report_period);
    let ping_period = Duration::from_secs(30);
    let mut ping = interval_at(now + ping_period, ping_period);
    report.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ping.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = report.tick() => {
                let frame = state.frame();
                if let Some(frame) = frame {
                    send_agent(socket, &AgentMessage::Metrics(frame)).await?;
                }
            }
            _ = state.ready.notified() => { send_pending(socket, state).await?; }
            _ = ping.tick() => { send(socket, Message::Ping(Default::default())).await?; }
            message = receive(socket) => match message? {
                HubMessage::Ack { minute_ms } => state.queue.borrow_mut().ack(minute_ms),
                HubMessage::Welcome { .. } => return Err(End::Retry("unexpected duplicate Welcome")),
            },
        }
    }
}

#[cfg(test)]
mod tests;
