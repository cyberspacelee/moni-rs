use std::num::NonZeroUsize;

use clap::Parser;

#[derive(Parser)]
#[command(version, about = "moni-rs VPS agent")]
pub struct Config {
    /// Hub endpoint, e.g. wss://moni.example.workers.dev/ws/agent
    #[arg(long, env = "MONI_SERVER")]
    pub server: String,
    /// Agent token
    #[arg(long, env = "MONI_TOKEN", hide_env_values = true)]
    pub token: String,
    /// Preferred report interval in seconds; Hub Welcome overrides this value
    #[arg(long, env = "MONI_INTERVAL", default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    pub interval: u32,
    /// Interface allowlist, overriding virtual-interface filtering
    #[arg(long, env = "MONI_IFACES", value_delimiter = ',', value_parser = list_item)]
    pub ifaces: Vec<String>,
    /// Mount-point allowlist, overriding physical-device filtering
    #[arg(long, env = "MONI_DISKS", value_delimiter = ',', value_parser = list_item)]
    pub disks: Vec<String>,
    /// Maximum number of unacknowledged minutes kept in memory
    #[arg(long, env = "MONI_QUEUE_CAPACITY", default_value = "1440")]
    pub queue_capacity: NonZeroUsize,
}

fn list_item(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("allowlist entries must not be empty".into())
    } else {
        Ok(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_allowlists_and_rejects_zero_limits() {
        let args = [
            "agent",
            "--server",
            "ws://localhost/ws/agent",
            "--token",
            "test",
        ];
        let config = Config::try_parse_from(args.into_iter().chain([
            "--ifaces",
            "eth0, wg0",
            "--disks",
            "/,/data",
        ]))
        .unwrap();
        assert_eq!(config.ifaces, ["eth0", "wg0"]);
        assert_eq!(config.disks, ["/", "/data"]);
        assert!(Config::try_parse_from(args.into_iter().chain(["--interval", "0"])).is_err());
        assert!(Config::try_parse_from(args.into_iter().chain(["--queue-capacity", "0"])).is_err());
    }
}
