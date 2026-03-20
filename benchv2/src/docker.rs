use anyhow::Result;
use bollard::Docker;
use std::collections::HashMap;

/// Connect to the Docker daemon via the Unix socket.
pub fn connect() -> Result<Docker> {
    Ok(Docker::connect_with_unix_defaults()?)
}

/// Get IPs of all containers with a given compose service label.
pub async fn get_service_ips(docker: &Docker, service: &str) -> Result<Vec<String>> {
    let filters = HashMap::from([(
        "label".to_string(),
        vec![format!("com.docker.compose.service={service}")],
    )]);
    let containers = docker
        .list_containers(Some(bollard::container::ListContainersOptions {
            filters,
            ..Default::default()
        }))
        .await?;

    let mut ips = Vec::new();
    for c in containers {
        if let Some(net_settings) = c.network_settings {
            if let Some(networks) = net_settings.networks {
                for (_name, net) in networks {
                    if let Some(ip) = net.ip_address {
                        if !ip.is_empty() {
                            ips.push(ip);
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(ips)
}
