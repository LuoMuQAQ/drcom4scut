# 本地补丁记录

此文件记录对上游 `drcom4scut-0.3.2` 的本地修改。

## 1. supervisor.rs - 父进程监视与 Logoff

**修改时间：** 0.3.4 之前  
**位置：** `src/supervisor.rs`  
**目的：** GUI 关闭时通过监视父进程和停止事件，发送 EAPOL-Logoff 后退出。

## 2. main.rs - 环境变量传递

**修改时间：** 0.3.4 之前  
**位置：** `src/main.rs`  
**目的：** 接收 `DRCOM_PARENT_HANDLE` 和 `DRCOM_SHUTDOWN_EVENT` 环境变量。

## 3. 0.3.4 重编译

**修改时间：** 2026-09-23  
**SHA-256：** `1838ad6c20c99d84d8a6d2ee603dda133a2314209b65ed9cdd31d0247c8f1052`  
**说明：** 重新编译以支持优雅下线，版本号仍为 0.3.2。

## 4. 深度稳定性与性能优化 (0.3.5)

**修改时间：** 2026-09-23  
**目的：** 修复睡眠唤醒、网络错误处理、死锁风险等稳定性问题。

### 主要改动：

1. **socket.rs** - 添加 Socket 超时和网络就绪检测
2. **udp.rs** - 优化 RwLock 争用，改进错误恢复
3. **eap.rs** - 改进错误处理
4. **device.rs** - 添加设备状态验证和重建机制
5. **util.rs** - 优化自旋等待策略
6. **main.rs** - 改进设备重新获取策略

## 5. 核心稳定性与鲁棒性修复 (0.3.6)

**修改时间：** 2026-09-25  
**SHA-256：** `6bdcedd20e30ae9721a7db95c8d8dbf6dd01f57e9ed50fa4b8583b2c7d16938f`  
**说明：** 修复睡眠唤醒掉线无法自愈、工作线程挂掉后进程自愈、畸形报文防崩溃等稳定性问题，版本号仍严格保持 `drcom4scut 0.3.2`。

### 主要改动：

1. **main.rs / Cargo.toml** - panic 策略调整为 `unwind` 并用 `catch_unwind` 隔离，避免工作线程 panic 导致主进程硬崩溃。
2. **udp.rs / main.rs / eap.rs** - UDP 线程重建时自动重注入缓存的 EAP SUCCESS 状态，缓解 UDP 等待 EAP SUCCESS 的恢复问题；旧接收线程竞争仍存在，见下方后续修复。
3. **udp.rs / socket.rs** - 修复重发线程在空队列或异常时的 busy-spin 空转；修复读超时被误报为致命错误的问题。
4. **packet.rs / udp.rs / eap.rs** - 强化报文解析防越界防畸形，增加长度校验与边界防护。
5. **udp.rs** - RwLock 获取失败时引入有限重试策略，避免静默丢包或状态滞后。
6. **udp.rs** - UDP 心跳连续超时后触发主动重启重建。
7. **eap.rs** - 区分 EAP 消息类型，非账号类致命错误的通知不再直接断开会话。
8. **main.rs** - 日志目录缺失时尝试自动创建而不崩溃退出；增加死线程探针检测与自动重建。
9. **config.rs** - 对危险配置参数执行边界 clamp 校正，避免极端参数异常。


## 6. HeroUI 发行修订的 UDP 重连修复 (0.3.6)

2026-09-25。native GUI 内嵌核心 SHA-256：`e571bf8b366db40c43a0b4e17e8faaafe2088c242ede4856995965b2868d5083`；历史 .NET 核心资源保持原样。

每代 UDP 独立 EAP inbox，缓存重放与发布同步；STOP/SLEEP/QUIT 和 EAP 失败/退出使缓存失效。UDP owner 直接接收通知，移除旧 EAPtoUDP 线程；清理旧 checksum、单调超时与取消、Drop 唤醒及空闲线程退出。13 项离线测试通过，真实联网恢复待验证，Windows10022 底层原因未确定。

维护源码及详细补丁见 `vendor/drcom4scut-0.3.2/PATCHES.md`。`build-core.ps1` 在该版默认生成 native 资源并校验该版哈希。

## 7. Windows GUI 0.3.9 内嵌核心

EAP/UDP 空闲等待改为事件驱动，保留 10 ms 工作线程检查兜底。更新包在 EAP 接收线程提前跳过非外层 EAPOL；不改驱动、认证报文、GUI 日志判定或 Logoff。核心自身版本仍为 `0.3.2`。

本版 native GUI 内嵌核心（v0.3.9-appfilter）SHA-256：`ac790ca75ee97366096d6e45b129a18e8b5cdc726c28eefef17c7608bb7aff32`。

## 8. Npcap 内核 cBPF + 事件驱动读（2026-11-24）

**位置：** `src/npcap.rs`（新增，仅 Windows）、`src/device.rs`、`src/eap.rs`、`src/main.rs`
**目的：** 把 EAPOL 过滤下推到 Npcap 内核 cBPF，并用驱动读事件替代空读；应用层仍保留外层 `0x888e` 兜底。

- Windows `Device::with_ip_net` 优先调用 `npcap::open_channel`：8 条 cBPF 只接受外层 `0x888e` 且目的 MAC 为本机的帧；`PacketSetBuff`、用户读缓冲和 `PacketSetMinToCopy(1)` 固定为 64 KiB / 1；`PacketSetHwFilter` 优先 `DIRECTED|MULTICAST`，失败回退 `PROMISCUOUS`。
- 内核过滤安装成功时禁用 NPF 回环、`PacketSetReadTimeout(-1)`，接收线程等待 `PacketGetReadEvent`；超时只检查停止事件，不调用 `PacketReceivePacket`。`PacketGetReadEvent` 或 `PacketSetBpf` 不可用时记录 `warn!` 并回退到 250 ms 直接读与应用层过滤。
- `Device::wake_receiver` 在 EAP 停止、退出和 `Drop` 时唤醒接收线程；`receive_eapol_frame` 将 `Interrupted` 映射为 `Ok(None)`，避免停止时泄漏 Npcap 句柄。
- 认证报文、UDP 心跳、EAP 状态机、GUI 日志判定和正式 Logoff 路径不变；`PacketSetSnapLen` 不调用，截断长度由 BPF `RET #65535` 控制。
- 离线单测覆盖 ABI、软 BPF（短帧/IPv4/VLAN/非本机目的 MAC/本机 EAPOL）、`bpf_hdr` 边界、超时状态和中断唤醒。2026-11-24 本机 A1/B1/B2/A2 各 60 秒 ABBA 与 90 秒冷启动验收已通过：NPF 读取降幅 ≥99.999%，读字节降幅 99.9999%，`bs_capt` 降幅 ≥99.999%，`bs_drop` 为 0，核心单核 CPU 分别降低 92.4% / 95.2%；冷启动首次认证 7.33 秒，回复 2 次 EAP 心跳，退出码 0。

**发布版本：** Windows GUI 0.3.10（源码标签 `windows-v0.3.10`）
本版 native GUI 内嵌核心 SHA-256：`21b56c6684ee6bcbf37e41479eaf999ff8d82afc5cc9adb1d5b57e3c2cc39b73`。
