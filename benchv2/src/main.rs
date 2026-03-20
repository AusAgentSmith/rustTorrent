mod bencode;
mod charts;
mod clients;
mod config;
mod datagen;
mod docker;
mod metrics;
mod mock_seeder;
mod report;
mod runner;
mod tracker;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "benchv2", about = "BitTorrent client benchmark: rqbit vs qBittorrent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the benchmark orchestrator
    Run {
        /// Scenario group or comma-separated names
        #[arg(long, default_value = "quick", env = "SCENARIOS")]
        scenarios: String,
        /// Data directory for test files
        #[arg(long, default_value = "/data/testdata")]
        data_dir: PathBuf,
        /// Torrent file directory
        #[arg(long, default_value = "/data/torrents")]
        torrent_dir: PathBuf,
        /// Results output directory
        #[arg(long, default_value = "/results")]
        results_dir: PathBuf,
    },
    /// Run the HTTP BitTorrent tracker
    Tracker {
        #[arg(long, default_value = "6969")]
        port: u16,
    },
    /// Run the mock BitTorrent seeder
    MockSeed {
        #[arg(long, default_value = "100", env = "MOCK_PEERS")]
        peers: usize,
        #[arg(long, default_value = "6900")]
        base_port: u16,
        #[arg(long, default_value = "http://tracker:6969/announce")]
        tracker_url: String,
        #[arg(long, default_value = "/data/testdata")]
        data_dir: PathBuf,
        #[arg(long, default_value = "/data/torrents")]
        torrent_dir: PathBuf,
        /// Health check port
        #[arg(long, default_value = "8080")]
        health_port: u16,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        match cli.command {
            Command::Run {
                scenarios,
                data_dir,
                torrent_dir,
                results_dir,
            } => {
                runner::run(scenarios, data_dir, torrent_dir, results_dir).await
            }
            Command::Tracker { port } => tracker::run(port).await,
            Command::MockSeed {
                peers,
                base_port,
                tracker_url,
                data_dir,
                torrent_dir,
                health_port,
            } => {
                mock_seeder::run(peers, base_port, tracker_url, data_dir, torrent_dir, health_port)
                    .await
            }
        }
    })
}
