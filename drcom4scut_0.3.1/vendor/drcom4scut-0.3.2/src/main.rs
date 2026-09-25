#![feature(ip)]
mod device;
mod eap;
mod eap_relay;
mod logger;
mod settings;
mod socket;
mod supervisor;
mod udp;
mod util;

use log::{error, info};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::settings::Settings;
use crate::socket::Socket;
use crate::util::{ChannelData, State, sleep_at};

fn main() {
    let settings = &settings::SETTINGS;

    logger::init(settings);
    supervisor::arm();

    info!("Start to run...");
    let device =
        device::get_device(settings.mac, settings.ip).expect("Fail on getting ethernet device!");
    supervisor::set_logoff_mac(device.mac);
    info!("Ethernet Device: {}", &device.interface.name);
    info!("MAC address: {}", &device.mac);
    info!("IP Address/Prefix: {}", &device.ip_net);
    info!("Username: {}", settings.username);
    for dns in &settings.dns {
        info!("DNS Server: {dns}");
    }
    info!("Host: {}", settings.host);
    info!("Hostname: {}", settings.hostname);
    info!("Time to wake up: {}", settings.time);
    info!("Reconnect Interval: {}s", settings.reconnect);
    info!(
        "Heartbeat timeout of EAP: {}s",
        settings.heartbeat.eap_timeout
    );
    info!(
        "Heartbeat timeout of UDP: {}s",
        settings.heartbeat.udp_timeout
    );
    info!("Retry Count: {}", settings.retry.count);
    info!("Retry Interval: {}ms", settings.retry.interval);

    #[cfg(feature = "log4rs")]
    {
        info!("Log to console: {}", settings.log.enable_console);
        info!("Log to file: {}", settings.log.enable_file);
        info!("Log File Directory: {}", settings.log.file_directory);
        info!("Log Level: {}", settings.log.level_filter);
    }

    let mac = device.mac;
    let ip = device.ip_net.ip();

    let (eap_tx, eap_rx) = crossbeam_channel::unbounded::<ChannelData>();
    let relay = Arc::new(eap_relay::EapRelay::default());

    let _eap_handle = thread::Builder::new()
        .name("EAP-Process-Generator".to_owned())
        .spawn(move || {
            let device = Arc::new(device);
            let mut broke = false;
            loop {
                let mut device = device.clone();
                if broke {
                    info!("Try get the property ethernet device.");
                    let mut retry_delay = 1u64; // 从1秒开始
                    loop {
                        match device::get_device(Some(mac), Some(ip)) {
                            Ok(d) => {
                                device = Arc::new(d);
                                info!("Successfully reacquired ethernet device.");
                                break;
                            }
                            Err(e) => {
                                error!("Can't get ethernet device, try again in {} second(s) : {}", retry_delay, e);
                                thread::sleep(Duration::from_secs(retry_delay));
                                // 指数退避，最大15秒
                                retry_delay = (retry_delay * 2).min(settings.reconnect);
                            }
                        }
                    }
                }
                let tx = eap_tx.clone();
                thread::Builder::new()
                    .name("EAP-Process".to_owned())
                    .spawn(move || {
                        info!("Create EAP Process.");
                        let mut eap_process = eap::Process::new(settings, device, tx.clone());
                        info!("Start EAP Process.");
                        loop {
                            match eap_process.start() {
                                State::Sleep => {
                                    error!("Will try reconnect at the next {}.", settings.time);
                                    sleep_at(settings.time);
                                    continue;
                                }
                                State::Quit => {
                                    break;
                                }
                                _ => {
                                    // Some server notifications stop EAP without
                                    // publishing an explicit event to UDP.
                                    let _ = tx.send(ChannelData {
                                        state: State::Stop,
                                        data: Vec::new(),
                                    });
                                    error!(
                                        "Failed at 802.1X Authorization! Will try reconnect in {} second(s).",
                                        settings.reconnect
                                    );
                                }
                            }
                            thread::sleep(Duration::from_secs(settings.reconnect));
                        }
                        info!("Quit EAP Process.");
                    })
                    .expect("Can't create EAP Process thread!")
                    .join()
                    .unwrap_or_else(|_| error!("EAP Process thread panicked! Will restart."));

                // A panicked EAP process may not have sent QUIT. Invalidate
                // its cached session before retrying authentication.
                let _ = eap_tx.send(ChannelData { state: State::Stop, data: Vec::new() });
                error!(
                    "Fatal error at EAP Process thread! Will try restart in {} second(s).",
                    settings.reconnect
                );
                thread::sleep(Duration::from_secs(settings.reconnect));
                broke = true;
            }
        })
        .expect("Can't create EAP Process generator thread!");

    let udp_handle = {
        let relay = relay.clone();
        thread::Builder::new()
            .name("UDP-Process-Generator".to_owned())
            .spawn(move || {
                loop {
                    // Subscribe atomically with replay into this generation's
                    // own inbox; a retired process can never consume its events.
                    let rx = relay.subscribe();
                    thread::Builder::new()
                        .name("UDP-Process".to_owned())
                        .spawn(move || {
                            let (udp_ip, dns) = match socket::resolve_dns(settings) {
                                Some(r) => r,
                                None => {
                                    error!("UDP: Can't resolve '{}'.", settings.host);
                                    return;
                                }
                            };
                            let socket = Socket::new(match socket::socket_bind(udp_ip) {
                                Some(socket) => socket,
                                None => {
                                    error!("UDP: Can't create socket and connect to '{udp_ip}'.");
                                    return;
                                }
                            });
                            info!("Create UDP Process.");
                            let mut udp_process = udp::Process::new(
                                settings,
                                Arc::new(socket),
                                rx,
                                mac,
                                ip,
                                dns,
                            );
                            info!("Start UDP Process.");
                            loop {
                                match udp_process.start() {
                                    State::Sleep => {
                                        error!(
                                            "Will try restart UDP heartbeat at the next {}.",
                                            settings.time
                                        );
                                        sleep_at(settings.time);
                                        continue;
                                    }
                                    State::Quit => {
                                        break;
                                    }
                                    _ => {
                                        error!(
                                            "Failed at UDP Process! Will try reconnect in {} second(s).",
                                            settings.reconnect
                                        );
                                    }
                                }
                                thread::sleep(Duration::from_secs(settings.reconnect));
                            }
                            info!("Quit UDP Process.");
                        })
                        .expect("Can't create UDP Process thread!")
                        .join()
                        .unwrap_or_else(|_| error!("UDP Process thread panicked! Will restart."));
                    error!(
                        "Fatal error at UDP Process thread! Will try restart in {} second(s).",
                        settings.reconnect
                    );
                    thread::sleep(Duration::from_secs(settings.reconnect));
                }
            })
            .expect("Can't create UDP Process generator thread!")
    };

    // Retain the latest authentication state even while UDP is rebuilding.
    while let Ok(msg) = eap_rx.recv() {
        relay.publish(msg);
    }

    if udp_handle.join().is_err() {
        error!("Fatal error! UDP Process generator thread panicked!");
    }
    if _eap_handle.join().is_err() {
        error!("Fatal error! EAP Process generator thread panicked!");
    }
}
