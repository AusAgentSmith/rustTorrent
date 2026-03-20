use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Default)]
pub struct MetricSample {
    pub ts: f64,
    pub cpu_pct: f64,
    pub mem_bytes: u64,
    pub net_rx_bps: f64,
    pub net_tx_bps: f64,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    pub iowait_pct: f64,
}

pub struct MetricsCollector {
    prom_url: String,
    http: reqwest::Client,
    container_ids: HashMap<String, String>,
}

impl MetricsCollector {
    pub fn new(prom_url: &str) -> Self {
        Self {
            prom_url: prom_url.to_string(),
            http: reqwest::Client::new(),
            container_ids: HashMap::new(),
        }
    }

    pub async fn resolve_container_id(&mut self, service: &str, docker: &bollard::Docker) {
        if self.container_ids.contains_key(service) {
            return;
        }
        let filters = std::collections::HashMap::from([(
            "label".to_string(),
            vec![format!("com.docker.compose.service={service}")],
        )]);
        if let Ok(containers) = docker
            .list_containers(Some(bollard::container::ListContainersOptions {
                filters,
                ..Default::default()
            }))
            .await
        {
            if let Some(c) = containers.first() {
                if let Some(id) = &c.id {
                    self.container_ids.insert(service.to_string(), id.clone());
                }
            }
        }
    }

    fn container_filter(&self, service: &str) -> String {
        if let Some(cid) = self.container_ids.get(service) {
            format!(r#"id=~".*{}.*""#, &cid[..12])
        } else {
            format!(
                r#"container_label_com_docker_compose_service="{}""#,
                service
            )
        }
    }

    async fn query_range(
        &self,
        query: &str,
        start: f64,
        end: f64,
    ) -> Vec<(f64, f64)> {
        let resp = self
            .http
            .get(format!("{}/api/v1/query_range", self.prom_url))
            .query(&[
                ("query", query),
                ("start", &start.to_string()),
                ("end", &end.to_string()),
                ("step", "1s"),
            ])
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Prometheus query failed: {e}");
                return vec![];
            }
        };

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(_) => return vec![],
        };

        let results = body
            .pointer("/data/result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if results.is_empty() {
            return vec![];
        }

        results[0]
            .get("values")
            .and_then(|v| v.as_array())
            .map(|vals| {
                vals.iter()
                    .filter_map(|pair| {
                        let arr = pair.as_array()?;
                        let ts = arr.first()?.as_f64()?;
                        let val: f64 = arr.get(1)?.as_str()?.parse().ok()?;
                        Some((ts, val))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn collect(
        &self,
        service: &str,
        start: f64,
        end: f64,
    ) -> Vec<MetricSample> {
        let f = self.container_filter(service);
        let cpu_f = format!("{f},cpu=\"total\"");

        // Pre-build query strings (need stable borrows for tokio::join!)
        let q_cpu = format!("rate(container_cpu_usage_seconds_total{{{cpu_f}}}[2s]) * 100");
        let q_mem = format!("container_memory_working_set_bytes{{{f}}}");
        let q_rx = format!("rate(container_network_receive_bytes_total{{{f}}}[2s])");
        let q_tx = format!("rate(container_network_transmit_bytes_total{{{f}}}[2s])");
        let q_dr = format!("rate(container_fs_reads_bytes_total{{{f}}}[2s])");
        let q_dw = format!("rate(container_fs_writes_bytes_total{{{f}}}[2s])");
        let q_io = r#"avg(rate(node_cpu_seconds_total{mode="iowait"}[2s])) * 100"#;

        // Fire all queries concurrently
        let (cpu, mem, net_rx, net_tx, disk_r, disk_w, iowait) = tokio::join!(
            self.query_range(&q_cpu, start, end),
            self.query_range(&q_mem, start, end),
            self.query_range(&q_rx, start, end),
            self.query_range(&q_tx, start, end),
            self.query_range(&q_dr, start, end),
            self.query_range(&q_dw, start, end),
            self.query_range(q_io, start, end),
        );

        // Collect all timestamps
        let mut all_ts: Vec<f64> = vec![];
        for series in [&cpu, &mem, &net_rx, &net_tx, &disk_r, &disk_w, &iowait] {
            for &(ts, _) in series {
                all_ts.push(ts);
            }
        }
        all_ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        all_ts.dedup_by(|a, b| (*a - *b).abs() < 0.8);

        let lookup = |series: &[(f64, f64)], ts: f64| -> f64 {
            series
                .iter()
                .find(|(t, _)| (t - ts).abs() < 3.0)
                .map(|(_, v)| *v)
                .unwrap_or(0.0)
        };

        all_ts
            .iter()
            .map(|&ts| MetricSample {
                ts,
                cpu_pct: lookup(&cpu, ts),
                mem_bytes: lookup(&mem, ts) as u64,
                net_rx_bps: lookup(&net_rx, ts),
                net_tx_bps: lookup(&net_tx, ts),
                disk_read_bps: lookup(&disk_r, ts),
                disk_write_bps: lookup(&disk_w, ts),
                iowait_pct: lookup(&iowait, ts),
            })
            .collect()
    }
}
