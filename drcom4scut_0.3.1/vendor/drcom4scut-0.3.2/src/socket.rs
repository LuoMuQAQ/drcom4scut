use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use log::{error, info};
use trust_dns_resolver::Resolver;
use trust_dns_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};

use crate::settings::Settings;

pub fn resolve_dns(settings: &Settings) -> Option<(IpAddr, SocketAddr)> {
    info!("DNS resolving...");
    let r = settings
        .dns
        .iter()
        .filter_map(|address| {
            let mut config = ResolverConfig::new();
            config.add_name_server(NameServerConfig {
                socket_addr: *address,
                protocol: Protocol::Udp,
                tls_dns_name: None,
                trust_negative_responses: false,
                bind_addr: None,
            });
            info!("Use DNS: {}:{}", address.ip(), address.port());
            let resolver = match Resolver::new(config, ResolverOpts::default()) {
                Ok(r) => r,
                Err(_) => {
                    error!("Failed to connect resolver.");
                    return None;
                }
            };
            let lookup = match resolver.lookup_ip(&settings.host) {
                Ok(r) => r,
                Err(_) => {
                    error!("Failed to lookup.");
                    return None;
                }
            };
            if let Some(ip) = lookup.iter().next() {
                Some((ip, *address))
            } else {
                error!("No addresses returned!");
                None
            }
        })
        .next();
    info!("Resolve result:");
    if let Some(r1) = r {
        info!("IP: {}", &r1.0);
    } else {
        error!("Resolve failed.");
    }
    r
}

pub fn socket_bind(ip: IpAddr) -> Option<UdpSocket> {
    use std::thread;
    use std::time::Duration;
    
    let mut port = 36144;
    let address = SocketAddr::new(ip, 61440);
    let mut route_wait_attempts = 0;
    const MAX_ROUTE_WAITS: u32 = 3;
    
    loop {
        match UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)) {
            Ok(socket) => {
                match socket.connect(address) {
                    Ok(()) => return Some(socket),
                    Err(e) => {
                        // 区分错误类型：路由问题 vs 其他问题
                        let is_route_error = e.raw_os_error()
                            .map_or(false, |code| code == 10051 || code == 10065);

                        if is_route_error {
                            if route_wait_attempts < MAX_ROUTE_WAITS {
                                // 网络可能未就绪，等待后重试
                                route_wait_attempts += 1;
                                info!("Network route not ready (attempt {}), waiting...", route_wait_attempts);
                                thread::sleep(Duration::from_millis(500 * route_wait_attempts as u64));
                                continue;
                            }
                            // 路由仍不可达：扫描端口没有意义，直接放弃
                            error!("Network route not ready after {MAX_ROUTE_WAITS} attempts, give up.");
                            return None;
                        }
                        // 端口绑定成功但连接失败，尝试下一个端口
                    }
                }
            }
            Err(_) => {
                if port == u16::MAX {
                    return None;
                }
            }
        }
        port += 1;
        if port > 36144 + 1000 {
            // 防止无限循环，尝试1000个端口后放弃
            return None;
        }
    }
}

pub struct Socket {
    socket: UdpSocket,
}

impl Socket {
    pub fn new(socket: UdpSocket) -> Socket {
        // 设置读写超时，避免永久阻塞
        let _ = socket.set_read_timeout(Some(std::time::Duration::from_secs(30)));
        let _ = socket.set_write_timeout(Some(std::time::Duration::from_secs(5)));
        Socket { socket }
    }

    pub fn send(&self, data: Vec<u8>) -> io::Result<()> {
        let l = data.len();
        let mut n = 0;
        let max_attempts = 3;
        let mut attempts = 0;
        while n < l {
            match self.socket.send(&data[n..l]) {
                Ok(sent) => {
                    n += sent;
                    attempts = 0;
                }
                Err(e) if attempts < max_attempts && Self::is_transient(&e) => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn receive(&self) -> io::Result<Vec<u8>> {
        let mut buffer = [0u8; 2048];
        let size = self.socket.recv(&mut buffer)?;
        let v = buffer[..size].to_vec();
        Ok(v)
    }

    fn is_transient(err: &io::Error) -> bool {
        matches!(
            err.kind(),
            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        )
    }

    pub fn is_valid(&self) -> bool {
        // local_addr() stays valid on a broken socket; SO_ERROR is a real probe.
        matches!(self.socket.take_error(), Ok(None))
    }
}
