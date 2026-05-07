use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// SSRF 防护：阻止对私有/内部网络的 HTTP 请求
///
/// 阻止范围（IPv4）：
///   0.0.0.0/8        "this" network
///   10.0.0.0/8       private
///   100.64.0.0/10    CGNAT / shared address space（部分云 metadata）
///   169.254.0.0/16   link-local（云 metadata）
///   172.16.0.0/12    private
///   192.168.0.0/16   private
///
/// 阻止范围（IPv6）：
///   ::               unspecified
///   fc00::/7         unique local
///   fe80::/10        link-local
///   ::ffff:<v4>      mapped IPv4 in blocked range
///
/// 允许（不阻止）：
///   127.0.0.0/8      loopback（本地开发 hook）
///   ::1              loopback
///   其他所有公网地址
pub fn check_url(url: &str) -> Result<(), String> {
    // 1. 解析 URL
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid URL".to_string())?;

    // 2. 提取 host
    let host = parsed
        .host_str()
        .ok_or_else(|| "No host in URL".to_string())?;

    // 3. DNS 解析
    let port = parsed.port().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);

    let addrs: Vec<SocketAddr> = match std::net::ToSocketAddrs::to_socket_addrs(&addr_str) {
        Ok(iter) => iter.collect(),
        Err(_) => return Err("DNS resolution failed".to_string()),
    };

    if addrs.is_empty() {
        return Err("DNS resolution returned no addresses".to_string());
    }

    // 4. 检查每个 IP
    for addr in &addrs {
        match addr.ip() {
            IpAddr::V4(ip) => {
                if is_blocked_ipv4(ip) {
                    return Err(format!("Blocked: {}", ip));
                }
            }
            IpAddr::V6(ip) => {
                if is_blocked_ipv6(ip) {
                    return Err(format!("Blocked: {}", ip));
                }
            }
        }
    }

    Ok(())
}

/// 检查 IPv4 地址是否在阻止范围内
fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    // 允许 loopback
    if ip.is_loopback() {
        return false;
    }

    use ipnet::Ipv4Net;
    let blocked_ranges: &[Ipv4Net] = &[
        "0.0.0.0/8".parse().unwrap(),      // "this" network
        "10.0.0.0/8".parse().unwrap(),     // private
        "100.64.0.0/10".parse().unwrap(),  // CGNAT / shared address space
        "169.254.0.0/16".parse().unwrap(), // link-local（云 metadata）
        "172.16.0.0/12".parse().unwrap(),  // private
        "192.168.0.0/16".parse().unwrap(), // private
    ];

    blocked_ranges.iter().any(|range| range.contains(&ip))
}

/// 检查 IPv6 地址是否在阻止范围内
fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    // 允许 loopback
    if ip.is_loopback() {
        return false;
    }

    // 检查 IPv4-mapped IPv6 地址（::ffff:<v4>）
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_ipv4(v4);
    }

    use ipnet::Ipv6Net;
    let blocked_ranges: &[Ipv6Net] = &[
        "::/0".parse().unwrap(), // unspecified (we only need to check specific blocked ranges)
        "fc00::/7".parse().unwrap(), // unique local
        "fe80::/10".parse().unwrap(), // link-local
    ];

    // :: (unspecified)
    if ip.is_unspecified() {
        return true;
    }

    blocked_ranges.iter().any(|range| range.contains(&ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_blocked_ipv4_private() {
        assert!(is_blocked_ipv4("10.0.0.1".parse().unwrap()));
        assert!(is_blocked_ipv4("192.168.1.1".parse().unwrap()));
        assert!(is_blocked_ipv4("172.16.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv4_metadata() {
        assert!(is_blocked_ipv4("169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ipv4("100.100.100.200".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv4_loopback_allowed() {
        assert!(!is_blocked_ipv4("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv4_this_network() {
        assert!(is_blocked_ipv4("0.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv6_private() {
        assert!(is_blocked_ipv6("fc00::1".parse().unwrap()));
        assert!(is_blocked_ipv6("fe80::1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv6_loopback_allowed() {
        assert!(!is_blocked_ipv6("::1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv6_unspecified() {
        assert!(is_blocked_ipv6("::".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_ipv4_mapped_ipv6() {
        // ::ffff:192.168.1.1 should be blocked
        let ip: Ipv6Addr = "::ffff:192.168.1.1".parse().unwrap();
        assert!(is_blocked_ipv6(ip));
    }

    #[test]
    fn test_is_blocked_ipv4_mapped_loopback_allowed() {
        // ::ffff:127.0.0.1 should be allowed (loopback)
        let ip: Ipv6Addr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(!is_blocked_ipv6(ip));
    }

    #[test]
    fn test_check_url_invalid_url() {
        assert!(check_url("not-a-url").is_err());
    }

    #[test]
    fn test_check_url_loopback_allowed() {
        // 127.0.0.1 should be allowed
        let result = check_url("http://127.0.0.1:8080");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_url_private_blocked() {
        // 192.168.x.x should be blocked
        let result = check_url("http://192.168.1.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_url_metadata_blocked() {
        // 169.254.169.254 should be blocked
        let result = check_url("http://169.254.169.254/latest/meta-data/");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_url_ipv6_loopback_allowed() {
        let result = check_url("http://[::1]:8080");
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_url_ipv6_private_blocked() {
        let result = check_url("http://[fc00::1]");
        assert!(result.is_err());
    }

    #[test]
    fn test_check_url_ipv4_mapped_blocked() {
        let result = check_url("http://[::ffff:192.168.1.1]");
        assert!(result.is_err());
    }
}
