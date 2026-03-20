use crate::clients::{qbittorrent::QBittorrentClient, rqbit::RqbitClient, transmission::TransmissionClient};
use crate::config::{self, Scenario, MB, GB};
use crate::metrics::{MetricSample, MetricsCollector};
use crate::{datagen, docker, report, charts};
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct ClientResult {
    pub client: String,
    pub scenario: String,
    pub scenario_description: String,
    pub total_bytes: u64,
    pub duration_sec: f64,
    pub avg_speed_mbps: f64,
    pub peak_speed_mbps: f64,
    pub time_to_first_piece: f64,
    pub cpu_avg: f64,
    pub cpu_peak: f64,
    pub mem_avg_mb: f64,
    pub mem_peak_mb: f64,
    pub net_rx_avg_mbps: f64,
    pub net_rx_peak_mbps: f64,
    pub disk_write_avg_mbps: f64,
    pub disk_write_peak_mbps: f64,
    pub iowait_avg: f64,
    pub iowait_peak: f64,
    pub timeseries: Vec<MetricSample>,
}

fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

async fn wait_for_service(name: &str, url: &str, timeout_secs: u64) -> Result<()> {
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("{name} not ready after {timeout_secs}s");
        }
        if let Ok(resp) = client
            .get(url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            if resp.status().as_u16() < 500 {
                tracing::info!("  {name}: ready");
                return Ok(());
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

pub async fn run(
    scenario_selector: String,
    data_dir: PathBuf,
    torrent_dir: PathBuf,
    results_dir: PathBuf,
) -> Result<()> {
    tracing::info!("============================================================");
    tracing::info!("  BitTorrent Client Benchmark: rqbit vs qBittorrent (Rust)");
    tracing::info!("============================================================");
    tracing::info!("Scenarios: {scenario_selector}");
    tracing::info!("Data dir: {}", data_dir.display());

    // Connect to Docker
    tracing::info!("Connecting to Docker...");
    let docker_client = docker::connect().map_err(|e| {
        tracing::error!("Failed to connect to Docker: {e}");
        tracing::error!("Make sure /var/run/docker.sock is mounted");
        e
    })?;
    tracing::info!("Docker connected.");

    // Init clients
    let rqbit = RqbitClient::new(config::RQBIT_API);
    let qbt = QBittorrentClient::new(config::QBT_API);
    let mut metrics = MetricsCollector::new(config::PROMETHEUS_API);

    // Wait for services
    tracing::info!("Waiting for services...");
    wait_for_service("tracker", "http://tracker:6969/health", 120).await?;
    wait_for_service("rqbit", &format!("{}/", config::RQBIT_API), 120).await?;
    wait_for_service("qBittorrent", &format!("{}/", config::QBT_API), 120).await?;
    wait_for_service("prometheus", &format!("{}/-/healthy", config::PROMETHEUS_API), 120).await?;

    // Discover seeders
    let seeder_ips = docker::get_service_ips(&docker_client, "seeder").await?;
    tracing::info!("Discovered {} seeder(s)", seeder_ips.len());

    let seeders: Vec<TransmissionClient> = seeder_ips
        .iter()
        .map(|ip| TransmissionClient::new(&format!("http://{ip}:9091/transmission/rpc")))
        .collect();

    // Verify seeder RPC
    let mut ok = 0;
    for s in &seeders {
        if s.get_torrents().await.is_ok() {
            ok += 1;
        }
    }
    tracing::info!("  Seeders: {ok}/{} RPC-ready", seeders.len());

    // Resolve container IDs for Prometheus
    metrics.resolve_container_id("rqbit", &docker_client).await;
    metrics
        .resolve_container_id("qbittorrent", &docker_client)
        .await;

    // Configure qBittorrent
    tracing::info!("Configuring qBittorrent...");
    qbt.authenticate(&docker_client).await?;
    qbt.configure_for_bench().await?;

    // Resolve scenarios
    let scenarios = config::resolve_scenarios(&scenario_selector);
    if scenarios.is_empty() {
        return Ok(());
    }

    let total_data: u64 = scenarios.iter().map(|s| s.total_bytes()).sum();
    tracing::info!(
        "Running {} scenario(s), {:.1} GB total data",
        scenarios.len(),
        total_data as f64 / GB as f64
    );
    for s in &scenarios {
        tracing::info!(
            "  {:30} {:>5} MB x {:>3} files = {:>6} MB  {:>4} peers  timeout={}s",
            s.name,
            s.file_size / MB,
            s.num_files,
            s.total_bytes() / MB,
            s.total_peers(),
            s.timeout_secs,
        );
    }

    // Generate data
    datagen::prepare_data(&scenarios, &data_dir, &torrent_dir).await?;

    // Run scenarios
    let mut all_results: Vec<(ClientResult, ClientResult)> = Vec::new();

    for sc in &scenarios {
        tracing::info!("============================================================");
        tracing::info!("SCENARIO: {} — {}", sc.name, sc.description);
        tracing::info!("============================================================");

        let tpaths = datagen::torrent_paths(sc, &torrent_dir);

        // Setup seeders
        if sc.real_seeders > 0 {
            tracing::info!("Setting up {} real seeder(s)...", sc.real_seeders);
            for s in &seeders {
                let _ = s.remove_all().await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            for tp in &tpaths {
                let tp_str = tp.to_string_lossy();
                for s in seeders.iter().take(sc.real_seeders) {
                    let _ = s.add_torrent(&tp_str, "/data/testdata").await;
                }
            }
            // Wait for seeding
            let verify_timeout = std::cmp::max(180, (sc.total_bytes() / (50 * MB)) as u64);
            tracing::info!("Waiting for seeders to verify (timeout {verify_timeout}s)...");
            let deadline =
                tokio::time::Instant::now() + std::time::Duration::from_secs(verify_timeout);
            loop {
                if tokio::time::Instant::now() > deadline {
                    tracing::error!("Seeders not ready — skipping scenario");
                    break;
                }
                let ready = futures_util::future::join_all(
                    seeders.iter().take(sc.real_seeders).map(|s| s.is_seeding()),
                )
                .await
                .into_iter()
                .filter(|&r| r)
                .count();
                if ready >= sc.real_seeders {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }

        if sc.mock_peers > 0 {
            tracing::info!("Mock seeder active with {} peers", sc.mock_peers);
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }

        tracing::info!("Seeding ready. Running downloads sequentially.");

        // Run rqbit
        let rqbit_result =
            run_client("rqbit", sc, &tpaths, &rqbit, &qbt, &metrics).await;
        cleanup_rqbit(&rqbit).await;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Run qBittorrent
        let qbt_result =
            run_client("qbittorrent", sc, &tpaths, &rqbit, &qbt, &metrics).await;
        cleanup_qbt(&qbt).await;

        // Clean seeders
        if sc.real_seeders > 0 {
            for s in &seeders {
                let _ = s.remove_all().await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        all_results.push((rqbit_result, qbt_result));
    }

    // Generate reports
    tokio::fs::create_dir_all(&results_dir).await?;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();

    report::write_json(&all_results, &results_dir, &timestamp)?;
    report::write_csv(&all_results, &results_dir, &timestamp)?;
    let summary = report::build_summary(&all_results);
    println!("\n{summary}");
    report::write_summary(&summary, &results_dir, &timestamp)?;

    let charts_dir = results_dir.join(format!("charts_{timestamp}"));
    std::fs::create_dir_all(&charts_dir)?;
    charts::generate_all(&all_results, &charts_dir)?;

    tracing::info!("Results: {}", results_dir.display());
    Ok(())
}

async fn run_client(
    client_name: &str,
    sc: &Scenario,
    tpaths: &[PathBuf],
    rqbit: &RqbitClient,
    qbt: &QBittorrentClient,
    metrics: &MetricsCollector,
) -> ClientResult {
    let mut result = ClientResult {
        client: client_name.to_string(),
        scenario: sc.name.clone(),
        scenario_description: sc.description.clone(),
        total_bytes: sc.total_bytes(),
        duration_sec: 0.0,
        avg_speed_mbps: 0.0,
        peak_speed_mbps: 0.0,
        time_to_first_piece: 0.0,
        cpu_avg: 0.0,
        cpu_peak: 0.0,
        mem_avg_mb: 0.0,
        mem_peak_mb: 0.0,
        net_rx_avg_mbps: 0.0,
        net_rx_peak_mbps: 0.0,
        disk_write_avg_mbps: 0.0,
        disk_write_peak_mbps: 0.0,
        iowait_avg: 0.0,
        iowait_peak: 0.0,
        timeseries: vec![],
    };

    tracing::info!("  [{client_name}] Adding {} torrent(s)...", tpaths.len());

    let mut rqbit_ids = Vec::new();
    if client_name == "rqbit" {
        for tp in tpaths {
            match rqbit.add_torrent(tp).await {
                Ok(id) => rqbit_ids.push(id),
                Err(e) => tracing::error!("  [rqbit] Failed to add {}: {e}", tp.display()),
            }
        }
    } else {
        for tp in tpaths {
            if let Err(e) = qbt.add_torrent(tp).await {
                tracing::error!("  [qbt] Failed to add {}: {e}", tp.display());
            }
        }
    }

    let start_time = now_unix();
    let start_instant = tokio::time::Instant::now();
    let mut first_piece_time: Option<f64> = None;
    let mut peak_speed: f64 = 0.0;
    let mut speeds = Vec::new();

    tracing::info!("  [{client_name}] Downloading (timeout {}s)...", sc.timeout_secs);

    let deadline = start_instant + std::time::Duration::from_secs(sc.timeout_secs);
    let mut timed_out = false;

    loop {
        if tokio::time::Instant::now() > deadline {
            tracing::warn!("  [{client_name}] TIMEOUT");
            timed_out = true;
            break;
        }

        let (finished, progress, speed) = if client_name == "rqbit" {
            let fin = rqbit.all_finished(&rqbit_ids).await.unwrap_or(false);
            let prog = rqbit.progress_fraction(&rqbit_ids).await;
            let spd = rqbit.aggregate_speed(&rqbit_ids).await;
            (fin, prog, spd)
        } else {
            let fin = qbt.all_finished().await.unwrap_or(false);
            let prog = qbt.progress_fraction().await;
            let spd = qbt.aggregate_speed().await;
            (fin, prog, spd)
        };

        if progress > 0.001 && first_piece_time.is_none() {
            first_piece_time = Some(start_instant.elapsed().as_secs_f64());
        }
        if speed > 0.0 {
            peak_speed = peak_speed.max(speed);
            speeds.push(speed);
        }

        if finished {
            tracing::info!("  [{client_name}] Complete!");
            break;
        }

        let bar_len = 30;
        let filled = (bar_len as f64 * progress) as usize;
        let bar: String = "#".repeat(filled) + &"-".repeat(bar_len - filled);
        let speed_mb = speed / MB as f64;
        eprint!(
            "\r  [{client_name}] [{bar}] {:5.1}% @ {:.1} MB/s",
            progress * 100.0,
            speed_mb
        );

        tokio::time::sleep(std::time::Duration::from_millis(config::POLL_INTERVAL_MS)).await;
    }
    eprintln!(); // newline after progress bar

    let end_time = now_unix();
    result.duration_sec = start_instant.elapsed().as_secs_f64();
    result.time_to_first_piece = first_piece_time.unwrap_or(result.duration_sec);
    result.peak_speed_mbps = peak_speed * 8.0 / 1_000_000.0;
    if !speeds.is_empty() {
        let avg = speeds.iter().sum::<f64>() / speeds.len() as f64;
        result.avg_speed_mbps = avg * 8.0 / 1_000_000.0;
    } else if result.duration_sec > 0.0 {
        result.avg_speed_mbps = result.total_bytes as f64 * 8.0 / result.duration_sec / 1_000_000.0;
    }

    // Collect Prometheus metrics
    tracing::info!("  [{client_name}] Collecting metrics...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let samples = metrics.collect(client_name, start_time, end_time).await;

    if !samples.is_empty() {
        let cpus: Vec<f64> = samples.iter().map(|s| s.cpu_pct).collect();
        let mems: Vec<u64> = samples.iter().map(|s| s.mem_bytes).collect();
        let rxs: Vec<f64> = samples.iter().map(|s| s.net_rx_bps).collect();
        let dws: Vec<f64> = samples.iter().map(|s| s.disk_write_bps).collect();
        let iow: Vec<f64> = samples.iter().map(|s| s.iowait_pct).collect();

        let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let max_f = |v: &[f64]| v.iter().cloned().fold(0.0f64, f64::max);
        let avg_u = |v: &[u64]| v.iter().sum::<u64>() as f64 / v.len().max(1) as f64;
        let max_u = |v: &[u64]| v.iter().cloned().max().unwrap_or(0);

        result.cpu_avg = avg(&cpus);
        result.cpu_peak = max_f(&cpus);
        result.mem_avg_mb = avg_u(&mems) / MB as f64;
        result.mem_peak_mb = max_u(&mems) as f64 / MB as f64;
        result.net_rx_avg_mbps = avg(&rxs) * 8.0 / 1e6;
        result.net_rx_peak_mbps = max_f(&rxs) * 8.0 / 1e6;
        result.disk_write_avg_mbps = avg(&dws) / MB as f64;
        result.disk_write_peak_mbps = max_f(&dws) / MB as f64;
        result.iowait_avg = avg(&iow);
        result.iowait_peak = max_f(&iow);
        result.timeseries = samples;
    }

    tracing::info!(
        "  [{client_name}] Done: {:.1}s, avg {:.1} Mbps",
        result.duration_sec,
        result.avg_speed_mbps
    );
    result
}

async fn cleanup_rqbit(rqbit: &RqbitClient) {
    tracing::info!("  [rqbit] Cleaning up...");
    let _ = rqbit.delete_all().await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    if let Ok(remaining) = rqbit.list_torrents().await {
        if !remaining.is_empty() {
            tracing::warn!("  [rqbit] {} torrent(s) still present, retrying...", remaining.len());
            let _ = rqbit.delete_all().await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }
}

async fn cleanup_qbt(qbt: &QBittorrentClient) {
    tracing::info!("  [qbittorrent] Cleaning up...");
    let _ = qbt.delete_all().await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
}
