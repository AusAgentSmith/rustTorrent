use crate::config::MB;
use crate::runner::ClientResult;
use anyhow::Result;
use std::path::Path;

const RQBIT_COLOR: &str = "#E45932";
const QBT_COLOR: &str = "#2681FF";
const BG_COLOR: &str = "#1a1a2e";
const PANEL_BG: &str = "#16213e";
const TEXT_COLOR: &str = "#ddd";
const GRID_COLOR: &str = "#333";

/// Generate all charts as SVGs.
pub fn generate_all(results: &[(ClientResult, ClientResult)], dir: &Path) -> Result<()> {
    tracing::info!("Generating charts...");
    for (rq, qb) in results {
        write_bar_chart(rq, qb, dir)?;
        write_timeseries(rq, qb, dir)?;
    }
    if results.len() > 1 {
        write_cross_scenario(results, dir)?;
    }
    write_dashboard(results, dir)?;
    let count = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == "svg"))
        .count();
    tracing::info!("  Generated {count} chart(s)");
    Ok(())
}

fn write_bar_chart(rq: &ClientResult, qb: &ClientResult, dir: &Path) -> Result<()> {
    let metrics: Vec<(&str, f64, f64)> = vec![
        ("Duration (s)", rq.duration_sec, qb.duration_sec),
        ("Avg Speed (Mbps)", rq.avg_speed_mbps, qb.avg_speed_mbps),
        ("Peak Speed (Mbps)", rq.peak_speed_mbps, qb.peak_speed_mbps),
        ("CPU Avg (%)", rq.cpu_avg, qb.cpu_avg),
        ("CPU Peak (%)", rq.cpu_peak, qb.cpu_peak),
        ("Mem Avg (MB)", rq.mem_avg_mb, qb.mem_avg_mb),
        ("Mem Peak (MB)", rq.mem_peak_mb, qb.mem_peak_mb),
        ("IO Wait (%)", rq.iowait_avg, qb.iowait_avg),
    ];
    let metrics: Vec<_> = metrics
        .into_iter()
        .filter(|(_, r, q)| *r != 0.0 || *q != 0.0)
        .collect();

    let n = metrics.len();
    let w = 900;
    let h = 60 + n * 50;
    let bar_w = 300.0;
    let gap = 50.0;
    let label_x = 150.0;

    let max_val = metrics
        .iter()
        .flat_map(|(_, r, q)| [*r, *q])
        .fold(0.0f64, f64::max)
        .max(1.0);

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" style="background:{BG_COLOR}">
<text x="{}" y="30" fill="{TEXT_COLOR}" font-size="16" text-anchor="middle" font-family="monospace">Metric Comparison — {}</text>
<text x="{}" y="48" fill="{RQBIT_COLOR}" font-size="11" font-family="monospace">■ rqbit</text>
<text x="{}" y="48" fill="{QBT_COLOR}" font-size="11" font-family="monospace">■ qBittorrent</text>"#,
        w / 2,
        rq.scenario,
        w - 200,
        w - 100,
    );

    for (i, (label, rq_v, qb_v)) in metrics.iter().enumerate() {
        let y = 70 + i * 50;
        let rq_w = rq_v / max_val * bar_w;
        let qb_w = qb_v / max_val * bar_w;
        let bar_x = label_x + gap;

        svg.push_str(&format!(
            r#"<text x="{label_x}" y="{}" fill="{TEXT_COLOR}" font-size="11" text-anchor="end" font-family="monospace">{label}</text>"#,
            y + 12,
        ));
        // rqbit bar
        svg.push_str(&format!(
            r#"<rect x="{bar_x}" y="{y}" width="{rq_w:.1}" height="18" fill="{RQBIT_COLOR}" opacity="0.9"/>"#,
        ));
        svg.push_str(&format!(
            r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="9" font-family="monospace">{:.1}</text>"#,
            bar_x + rq_w + 4.0,
            y + 13,
            rq_v,
        ));
        // qbt bar
        let y2 = y + 22;
        svg.push_str(&format!(
            r#"<rect x="{bar_x}" y="{y2}" width="{qb_w:.1}" height="18" fill="{QBT_COLOR}" opacity="0.9"/>"#,
        ));
        svg.push_str(&format!(
            r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="9" font-family="monospace">{:.1}</text>"#,
            bar_x + qb_w + 4.0,
            y2 + 13,
            qb_v,
        ));
    }

    svg.push_str("</svg>");
    let path = dir.join(format!("{}_comparison.svg", rq.scenario));
    std::fs::write(&path, &svg)?;
    Ok(())
}

fn write_timeseries(rq: &ClientResult, qb: &ClientResult, dir: &Path) -> Result<()> {
    let panels: Vec<(&str, Box<dyn Fn(&crate::metrics::MetricSample) -> f64>)> = vec![
        ("CPU %", Box::new(|s| s.cpu_pct)),
        ("Memory (MB)", Box::new(|s| s.mem_bytes as f64 / MB as f64)),
        ("IO Wait %", Box::new(|s| s.iowait_pct)),
    ];

    let panel_h = 150;
    let w = 800;
    let h = 60 + panels.len() * (panel_h + 30);

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" style="background:{BG_COLOR}">
<text x="{}" y="30" fill="{TEXT_COLOR}" font-size="16" text-anchor="middle" font-family="monospace">Time Series — {}</text>"#,
        w / 2,
        rq.scenario,
    );

    for (pi, (label, extractor)) in panels.iter().enumerate() {
        let py = 50 + pi * (panel_h + 30);
        let pw = w - 80;
        let px = 60;

        // Panel background
        svg.push_str(&format!(
            r#"<rect x="{px}" y="{py}" width="{pw}" height="{panel_h}" fill="{PANEL_BG}" rx="4"/>"#,
        ));
        svg.push_str(&format!(
            r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="12" text-anchor="middle" font-family="monospace">{label}</text>"#,
            px + pw / 2,
            py - 5,
        ));

        for (result, color) in [(rq, RQBIT_COLOR), (qb, QBT_COLOR)] {
            if result.timeseries.is_empty() {
                continue;
            }
            let t0 = result.timeseries[0].ts;
            let vals: Vec<(f64, f64)> = result
                .timeseries
                .iter()
                .map(|s| (s.ts - t0, extractor(s)))
                .collect();

            let max_t = vals.last().map(|(t, _)| *t).unwrap_or(1.0).max(1.0);
            let max_v = vals
                .iter()
                .map(|(_, v)| *v)
                .fold(0.0f64, f64::max)
                .max(0.01);

            let points: String = vals
                .iter()
                .map(|(t, v)| {
                    let x = px as f64 + (*t / max_t) * pw as f64;
                    let y = (py + panel_h) as f64 - (*v / max_v) * (panel_h - 10) as f64;
                    format!("{:.1},{:.1}", x, y)
                })
                .collect::<Vec<_>>()
                .join(" ");

            svg.push_str(&format!(
                r#"<polyline points="{points}" fill="none" stroke="{color}" stroke-width="2" opacity="0.9"/>"#,
            ));
        }
    }

    // Legend
    let ly = h - 15;
    svg.push_str(&format!(
        r#"<text x="60" y="{ly}" fill="{RQBIT_COLOR}" font-size="11" font-family="monospace">— rqbit</text>
<text x="160" y="{ly}" fill="{QBT_COLOR}" font-size="11" font-family="monospace">— qBittorrent</text>"#,
    ));

    svg.push_str("</svg>");
    let path = dir.join(format!("{}_timeseries.svg", rq.scenario));
    std::fs::write(&path, &svg)?;
    Ok(())
}

fn write_cross_scenario(results: &[(ClientResult, ClientResult)], dir: &Path) -> Result<()> {
    let n = results.len();
    let w = 900;
    let bar_h = 30;
    let h = 80 + n * (bar_h + 10) * 2 + 20;

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" style="background:{BG_COLOR}">
<text x="{}" y="30" fill="{TEXT_COLOR}" font-size="16" text-anchor="middle" font-family="monospace">Cross-Scenario: Speed Ratio</text>
<text x="{}" y="50" fill="{TEXT_COLOR}" font-size="10" font-family="monospace">&gt;1 = rqbit faster</text>"#,
        w / 2,
        w / 2,
    );

    let max_bar = 600.0;
    for (i, (rq, qb)) in results.iter().enumerate() {
        let y = 70 + i * (bar_h + 10);
        let ratio = if rq.duration_sec > 0.0 {
            qb.duration_sec / rq.duration_sec
        } else {
            1.0
        };
        let bar_w = (ratio / 2.0 * max_bar).min(max_bar);
        let color = if ratio >= 1.0 { "#4CAF50" } else { "#FF5722" };

        svg.push_str(&format!(
            r#"<text x="190" y="{}" fill="{TEXT_COLOR}" font-size="10" text-anchor="end" font-family="monospace">{}</text>"#,
            y + 20,
            rq.scenario,
        ));
        svg.push_str(&format!(
            r#"<rect x="200" y="{}" width="{bar_w:.0}" height="{bar_h}" fill="{color}" opacity="0.85" rx="3"/>"#,
            y + 5,
        ));
        svg.push_str(&format!(
            r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="10" font-family="monospace">{:.2}x</text>"#,
            200.0 + bar_w + 8.0,
            y + 23,
            ratio,
        ));
        // Reference line at 1.0
        let ref_x = 200.0 + (1.0 / 2.0 * max_bar);
        svg.push_str(&format!(
            "<line x1=\"{ref_x:.0}\" y1=\"{}\" x2=\"{ref_x:.0}\" y2=\"{}\" stroke=\"#888\" stroke-dasharray=\"4\" stroke-width=\"1\"/>",
            y + 5,
            y + 5 + bar_h,
        ));
    }

    svg.push_str("</svg>");
    std::fs::write(dir.join("cross_scenario.svg"), &svg)?;
    Ok(())
}

fn write_dashboard(results: &[(ClientResult, ClientResult)], dir: &Path) -> Result<()> {
    let n = results.len();
    let w = 1000;
    let h = 100 + n * 80;

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" style="background:{BG_COLOR}">
<text x="{}" y="30" fill="{TEXT_COLOR}" font-size="18" text-anchor="middle" font-family="monospace" font-weight="bold">Benchmark Dashboard: rqbit vs qBittorrent</text>
<text x="50" y="55" fill="{RQBIT_COLOR}" font-size="12" font-family="monospace">■ rqbit</text>
<text x="150" y="55" fill="{QBT_COLOR}" font-size="12" font-family="monospace">■ qBittorrent</text>
<text x="300" y="55" fill="{TEXT_COLOR}" font-size="10" font-family="monospace">Duration(s) | Speed(Mbps) | CPU Peak(%) | Mem Peak(MB)</text>"#,
        w / 2,
    );

    for (i, (rq, qb)) in results.iter().enumerate() {
        let y = 70 + i * 80;
        let max_dur = rq.duration_sec.max(qb.duration_sec).max(1.0);
        let max_spd = rq.avg_speed_mbps.max(qb.avg_speed_mbps).max(1.0);
        let max_cpu = rq.cpu_peak.max(qb.cpu_peak).max(0.1);
        let max_mem = rq.mem_peak_mb.max(qb.mem_peak_mb).max(1.0);

        // Scenario label
        svg.push_str(&format!(
            r#"<text x="10" y="{}" fill="{TEXT_COLOR}" font-size="10" font-family="monospace">{}</text>"#,
            y + 20,
            rq.scenario,
        ));

        let cols: Vec<(f64, f64, f64)> = vec![
            (rq.duration_sec, qb.duration_sec, max_dur),
            (rq.avg_speed_mbps, qb.avg_speed_mbps, max_spd),
            (rq.cpu_peak, qb.cpu_peak, max_cpu),
            (rq.mem_peak_mb, qb.mem_peak_mb, max_mem),
        ];

        for (ci, (rv, qv, maxv)) in cols.iter().enumerate() {
            let cx = 200 + ci * 200;
            let bw = 160.0;
            let rw = rv / maxv * bw;
            let qw = qv / maxv * bw;

            svg.push_str(&format!(
                r#"<rect x="{cx}" y="{}" width="{rw:.0}" height="14" fill="{RQBIT_COLOR}" opacity="0.9" rx="2"/>"#,
                y + 5,
            ));
            svg.push_str(&format!(
                r#"<rect x="{cx}" y="{}" width="{qw:.0}" height="14" fill="{QBT_COLOR}" opacity="0.9" rx="2"/>"#,
                y + 22,
            ));
            svg.push_str(&format!(
                r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="8" font-family="monospace">{:.1}</text>"#,
                cx as f64 + rw + 3.0,
                y + 15,
                rv,
            ));
            svg.push_str(&format!(
                r#"<text x="{}" y="{}" fill="{TEXT_COLOR}" font-size="8" font-family="monospace">{:.1}</text>"#,
                cx as f64 + qw + 3.0,
                y + 32,
                qv,
            ));
        }

        // Divider
        if i < n - 1 {
            svg.push_str(&format!(
                r#"<line x1="10" y1="{}" x2="{}" y2="{}" stroke="{GRID_COLOR}" stroke-width="1"/>"#,
                y + 55,
                w - 10,
                y + 55,
            ));
        }
    }

    svg.push_str("</svg>");
    std::fs::write(dir.join("dashboard.svg"), &svg)?;
    Ok(())
}
