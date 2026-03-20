## Issue Review: ikatson/rqbit (62 open)

Reviewed: 2026-03-20

### High Priority — Real Bugs

| # | Title | Why it matters |
|---|-------|----------------|
| **#525** | Memory leak / FD exhaustion | 34 comments, active debugging. Users hitting OOM/FD limits in production. |
| **#311** | Connection exhaustion (20k WAIT-CLOSE) | Service crashes after ~2 days. Related to #525. |
| **#561** | Checking pauses after disconnect, doesn't resume | Torrent stuck at 100% check, requires manual restart. |
| **#347** | Adding torrent hangs at 0% "checking files" | ~5% of the time torrents get stuck. |
| **#348** | Deleting torrent doesn't cancel "add torrent" API call | API call hangs forever. |
| **#363** | Download fully stops on temporary internet disruption | No automatic reconnection after network blip. |
| **#369** | File locked by process (Windows .exe files) | Can't run downloaded executables while session is active. |
| **#477** | Out of range integral type conversion | Panic/crash on certain torrents. |

### High Priority — Important Features / Protocol Compliance

| # | Title | Why it matters |
|---|-------|----------------|
| **#70 / #546** | BEP 52 (BitTorrent v2) | Major protocol evolution. #546 has a detailed design doc with 5 comments. CLAUDE.md already mandates v2 compliance. |
| **#160 / #539** | Missing Stopped/Completed announce events | Protocol violation — trackers can't track peers properly. #507 (wrong port) is related. Labeled `planned`. |
| **#493** | SOCKS proxy leaks (UDP trackers, DHT, etc.) | Privacy-critical for proxy users — defeats the purpose. |
| **#349** | Fast resume not used when adding torrents | Defeats the purpose of fast resume. Directly relevant to recent commit `1ea8623a`. |
| **#267** | Support tags / *arr app compatibility | Labeled `planned`, 7 comments. Would make rqbit usable with Sonarr/Radarr — large user demand. |

### Medium Priority — Useful Improvements

| # | Title | Notes |
|---|-------|-------|
| **#575** | Upgrade reqwest to 0.13 | Blocking downstream users from using librqbit as a library. Fresh (2026-03-19). |
| **#514** | Release new librqbit version | Downstream crate can't publish because it depends on git version. |
| **#350** | Limit memory usage | Only lever is `--max-blocking-threads` which also limits parallelism. |
| **#310** | Excessive DHT cache disk I/O | Significant I/O even with no active torrents. |
| **#553** | Settings change triggers full restart + recheck | Owner acknowledged, said fast resume algo needs tuning. |
| **#412** | Preallocation support | Reduces fragmentation on HDD. |
| **#484** | Don't create files for unselected torrent files | Creates empty files for pieces not being downloaded. |
| **#439** | Disable HTTP API by default (security) | 3 comments. Valid security concern for desktop app. |
| **#482** | HTTP API authentication (username/password) | No auth at all currently. |
| **#308** | Move files to different directory | Basic feature most clients have. |
| **#136** | Support read-only downloaded files | Crashes on read-only files instead of seeding. |
| **#346** | Set RUST_BACKTRACE=1 by default | Low-effort, helps debug rare crashes. |
| **#352** | Performance issues with blur CSS effect | Easy CSS fix for users without GPU acceleration. |

### Lower Priority — Nice to Have / Large Features

| # | Title | Notes |
|---|-------|-------|
| **#463** | BEP-55 Holepunch extension | NAT traversal for peers behind firewalls. |
| **#385** | BEP-46 Updating torrents via DHT | Only reference impl exists. Niche but interesting. |
| **#500** | Webseed support | Would help with HTTP-seeded torrents. |
| **#71 / #12** | WebRTC / WebTorrent | Two issues requesting the same thing. Would increase peer pool. |
| **#457 / #361** | Tor / I2P support | Privacy features, large scope. |
| **#491** | Native UI alternative | 10 comments. Users want GTK/Qt instead of web. |
| **#313** | Localization | i18n support. |
| **#440 / #318** | System tray minimize (Windows/Linux) | Standard desktop UX. |
| **#550** | QoS / niceness / priorities | Resource management. |
| **#551** | Import from other clients | Migration utility. |
| **#425** | UPnP/DLNA controller | Media server feature. |

### Can Probably Close

| # | Title | Notes |
|---|-------|-------|
| **#276** | How to set DHT listen_addr in Docker | Question, not bug. Likely answered. |
| **#306** | Will timeout with add_torrent cause issues? | Question. |
| **#473** | How to set custom User-Agent? | Question, 5 comments — likely resolved. |
| **#509** | How to seed pre-downloaded torrent? | Question, 4 comments. |
| **#327** | Example with resumable torrent | Question/docs, 8 comments. |
| **#354** | Store output_folder relative to home | Very niche. |
| **#343** | Unable to download specific torrent | Single torrent issue, may be tracker-side. |
| **#487** | 8.x releases larger than 9.x | Likely expected (different build config). |
| **#471** | Dependency issue with crates.io | Related to #514, may be resolved. |

### Recommendations

**Fix first** (high impact, likely fixable in this fork):
1. **#349** — Fast resume not working on add. Directly related to recent work.
2. **#539/#160** — Announce timing / missing events. Protocol correctness.
3. **#352** — CSS blur perf. One-line fix.
4. **#346** — RUST_BACKTRACE=1 default. Trivial.
5. **#348** — Cancel add-torrent on delete. UX bug.

**Watch closely** (likely affects users):
- **#525/#311** — Memory/connection leaks. 34 comments, actively investigated.
- **#363** — No reconnect after network blip.
- **#493** — Proxy leaks if using SOCKS.
