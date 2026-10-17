mod collector;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use moni_protocol::{AgentMessage, PROTOCOL_VERSION};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, http::HeaderValue};
use tracing::{info, warn};

use collector::Collector;

#[derive(Parser, Debug)]
#[command(version, about = "moni-rs VPS agent")]
struct Args {
    /// Hub endpoint, e.g. wss://moni.example.workers.dev/ws/agent
    #[arg(long, env = "MONI_SERVER")]
    server: String,
    /// Agent token
    #[arg(long, env = "MONI_TOKEN", hide_env_values = true)]
    token: String,
    /// Report interval in seconds
    #[arg(long, env = "MONI_INTERVAL", default_value_t = 5)]
    interval: u64,
    /// WebSocket protocol ping interval in seconds (free, does not wake the DO)
    #[arg(long, default_value_t = 30)]
    ping: u64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

async fn run_session(args: &Args, collector: &mut Collector) -> Result<()> {
    let mut req = args.server.as_str().into_client_request()?;
    req.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", args.token))?,
    );
    let (ws, resp) = tokio_tungstenite::connect_async(req)
        .await
        .context("connect")?;
    info!(status = %resp.status(), "connected");
    let (mut tx, mut rx) = ws.split();

    let hello = AgentMessage::Hello {
        protocol: PROTOCOL_VERSION,
        agent_version: env!("CARGO_PKG_VERSION").to_owned(),
        host: collector.host_info(),
    };
    tx.send(Message::text(serde_json::to_string(&hello)?))
        .await?;

    let mut report = tokio::time::interval(Duration::from_secs(args.interval.max(1)));
    let mut ping = tokio::time::interval(Duration::from_secs(args.ping.max(5)));
    // After a stall (suspend, SIGSTOP, overloaded host) do not burst out the missed reports.
    report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    report.tick().await; // first tick is immediate; it primes CPU/network baselines
    collector.sample(now_ms());
    ping.tick().await;

    loop {
        tokio::select! {
            _ = report.tick() => {
                let frame = collector.sample(now_ms());
                tx.send(Message::text(serde_json::to_string(&AgentMessage::Metrics(frame))?)).await?;
            }
            _ = ping.tick() => {
                tx.send(Message::Ping(Default::default())).await?;
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Close(frame))) => bail!("server closed: {frame:?}"),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => bail!("stream ended"),
            },
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args = Args::parse();
    let mut collector = Collector::new();

    let mut backoff = Duration::from_secs(1);
    loop {
        let started = std::time::Instant::now();
        tokio::select! {
            r = run_session(&args, &mut collector) => {
                if let Err(e) = r { warn!("session ended: {e:#}"); }
            }
            _ = tokio::signal::ctrl_c() => { info!("shutting down"); return Ok(()); }
        }
        // A session that lived a while resets the backoff.
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        let jitter = Duration::from_millis((now_ms() % 1000) as u64);
        info!("reconnecting in {:?}", backoff + jitter);
        tokio::time::sleep(backoff + jitter).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}
