use crate::transfer::types::SshHost;

/// Parse ~/.ssh/config and return a list of SshHost entries.
/// Skips wildcard hosts (Host *) and entries without a HostName.
pub fn parse_ssh_config() -> Vec<SshHost> {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    let path = format!("{home}/.ssh/config");

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => {
            log::info!("no ssh config found at {path}");
            return vec![];
        }
    };

    parse_ssh_config_content(&content)
}

fn parse_ssh_config_content(content: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut current: Option<SshHost> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Split on first whitespace or '='
        let (key, value) = match trimmed.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };

        match key.to_lowercase().as_str() {
            "host" => {
                // Save previous host if valid
                if let Some(host) = current.take() {
                    if !host.hostname.is_empty() {
                        hosts.push(host);
                    }
                }

                // Skip wildcards
                if value.contains('*') || value.contains('?') {
                    current = None;
                    continue;
                }

                current = Some(SshHost {
                    alias: value.to_string(),
                    port: 22,
                    ..Default::default()
                });
            }
            "hostname" => {
                if let Some(ref mut host) = current {
                    host.hostname = value.to_string();
                }
            }
            "user" => {
                if let Some(ref mut host) = current {
                    host.user = value.to_string();
                }
            }
            "port" => {
                if let Some(ref mut host) = current {
                    host.port = value.parse().unwrap_or(22);
                }
            }
            "identityfile" => {
                if let Some(ref mut host) = current {
                    // Expand ~ to HOME
                    let home = std::env::var("HOME").unwrap_or_default();
                    host.identity_file = value.replace('~', &home);
                }
            }
            _ => {}
        }
    }

    // Don't forget the last host
    if let Some(host) = current {
        if !host.hostname.is_empty() {
            hosts.push(host);
        }
    }

    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_config() {
        let config = r#"
Host myserver
    HostName 10.0.0.1
    User admin
    Port 2222

Host production
    HostName prod.example.com
    User deploy
    IdentityFile ~/.ssh/id_prod
"#;
        let hosts = parse_ssh_config_content(config);
        assert_eq!(hosts.len(), 2);

        assert_eq!(hosts[0].alias, "myserver");
        assert_eq!(hosts[0].hostname, "10.0.0.1");
        assert_eq!(hosts[0].user, "admin");
        assert_eq!(hosts[0].port, 2222);

        assert_eq!(hosts[1].alias, "production");
        assert_eq!(hosts[1].hostname, "prod.example.com");
        assert_eq!(hosts[1].user, "deploy");
        assert_eq!(hosts[1].port, 22);
    }

    #[test]
    fn skip_wildcard_hosts() {
        let config = r#"
Host *
    ServerAliveInterval 60

Host myserver
    HostName 10.0.0.1
"#;
        let hosts = parse_ssh_config_content(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "myserver");
    }

    #[test]
    fn skip_hosts_without_hostname() {
        let config = r#"
Host incomplete
    User admin

Host valid
    HostName 10.0.0.1
"#;
        let hosts = parse_ssh_config_content(config);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "valid");
    }

    #[test]
    fn empty_config() {
        let hosts = parse_ssh_config_content("");
        assert!(hosts.is_empty());
    }

    #[test]
    fn comments_and_blank_lines() {
        let config = r#"
# This is a comment

Host myserver
    # Another comment
    HostName 10.0.0.1
    User admin
"#;
        let hosts = parse_ssh_config_content(config);
        assert_eq!(hosts.len(), 1);
    }
}
