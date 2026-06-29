use bytesize::ByteSize;
use clap::Parser;
use humantime::parse_duration;

#[derive(Clone, Debug, Parser)]
#[command(name = "owt_streaming_service")]
#[command(about = "Stream 3D Tiles datasets")]
pub struct Config {
    /// Log level
    #[arg(long, env = "RUST_LOG", default_value = concat!(env!("CARGO_PKG_NAME"), "=info"))]
    pub log_level: String,

    /// Use pretty logging instead of JSON
    #[arg(long, env = "PRETTY_LOG")]
    pub pretty_log: bool,

    /// Listen address
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:3200")]
    pub listen_addr: String,

    /// Public base url
    #[arg(long, env = "BASE_URL")]
    pub base_url: String,

    /// Allow CORS from a specific origin, or "*" for any
    #[arg(long, env = "CORS_ORIGIN")]
    pub cors_origin: Option<String>,

    /// Prometheus metrics listen address
    #[arg(long, env = "METRICS_LISTEN_ADDR")]
    pub metrics_listen_addr: Option<String>,

    /// Location of layer configuration JSON documents
    #[arg(long, env = "LAYER_CONFIG_URI")]
    pub layer_config_uri: String,

    // How long should layer definitions stay in the cache?
    #[arg(long, env = "LAYER_DEFINITION_TTL", default_value = "5m", value_parser = parse_duration)]
    pub layer_definition_ttl: std::time::Duration,

    // Bound resource loader block cache
    #[arg(long, env, default_value = "2GiB")]
    pub block_cache_size: ByteSize,

    /// TLS certificate file path
    #[arg(long, env = "TLS_CERT", requires = "tls_key")]
    pub tls_cert: Option<String>,

    /// TLS private key file path
    #[arg(long, env = "TLS_KEY", requires = "tls_cert")]
    pub tls_key: Option<String>,
}

impl Config {
    pub fn load() -> Config {
        Config::parse()
    }
}
