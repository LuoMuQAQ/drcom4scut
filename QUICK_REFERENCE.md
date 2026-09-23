# drcom4scut 0.3.5 优化快速参考

## 🎯 核心改进

### 睡眠唤醒问题 - 已解决 ✅
- **Socket 超时：** 30秒自动返回，避免永久阻塞
- **路由检测：** 识别网络未就绪，等待而不是换端口
- **设备重获：** 指数退避 1→2→4→8→15秒
- **预期效果：** 重连时间从 30-60秒 降至 2-10秒

---

## 📂 修改文件清单

### 核心模块（需重新编译）
```
drcom4scut_0.3.1/vendor/drcom4scut-0.3.2/src/
├── socket.rs       ✅ Socket超时 + 路由检测 + 错误重试
├── util.rs         ✅ 智能自旋等待策略
├── main.rs         ✅ 指数退避设备重获
├── device.rs       ✅ 发送超时保护
└── udp.rs          ✅ RwLock超时 + Socket健康检查
```

### GUI模块
```
drcom4scut-rs/src/
└── install/setup_window.rs  ✅ 减少不必要的UI刷新
```

### 文档
```
├── drcom4scut_0.3.1/PATCHES.md      ✅ 补丁记录
├── OPTIMIZATION_SUMMARY.md          ✅ 优化总结
└── STABILITY_ANALYSIS.md            ✅ 深度分析报告
```

---

## 🔨 编译步骤

### 1. 编译核心
```powershell
cd drcom4scut_0.3.1
.\build-core.ps1
```

### 2. 验证核心哈希
```powershell
# 新的 SHA-256 将与之前不同
Get-FileHash .\src\Resources\drcom4scut.exe -Algorithm SHA256
```

### 3. 更新嵌入资源
```powershell
# 复制到 GUI 资源目录
Copy-Item .\src\Resources\drcom4scut.exe ..\drcom4scut-rs\resources\
```

### 4. 更新 GUI 中的哈希常量
编辑 `drcom4scut-rs/src/coreproc.rs`：
```rust
pub const CORE_SHA256: &str = "新的SHA256哈希值";
```

### 5. 编译 GUI
```powershell
cd ..\drcom4scut-rs
cargo build --release
cargo test
```

---

## 🧪 测试清单

### 基础功能测试
- [ ] 正常连接和断开
- [ ] 账号密码保存
- [ ] 网卡选择

### 稳定性测试
- [ ] **睡眠唤醒重连**（核心测试）
  - 睡眠 1 分钟 → 应在 2-10 秒内重连
  - 睡眠 30 分钟 → 应在 5-15 秒内重连
- [ ] 网线拔插
  - 拔出 30 秒 → 插回后 10 秒内恢复
- [ ] 长期运行
  - 24 小时连续运行 + 5 次睡眠唤醒
  - 监控内存和 CPU 使用率

### 性能测试
- [ ] 安装程序 CPU 使用率 < 1%
- [ ] 主程序 CPU 空闲时 < 0.5%
- [ ] UI 响应流畅，无卡顿

### 日志检查
查找关键字：
- ✅ "Successfully reacquired ethernet device" - 设备重获成功
- ✅ "Network route not ready" - 路由等待
- ⚠️ "Failed to acquire write lock" - 锁争用（偶尔出现正常）
- ❌ "Socket appears invalid" - Socket 失效（不应频繁出现）

---

## 🐛 常见问题排查

### Q1: 睡眠后仍然不重连
**检查：**
1. 查看日志是否有 "Receive error" 或 "Socket appears invalid"
2. 确认网络适配器驱动是否正常
3. 检查是否有防火墙或安全软件干扰

**解决：**
- 临时禁用防火墙测试
- 更新网卡驱动
- 尝试管理员权限运行

### Q2: 重连很慢（超过 15 秒）
**检查：**
1. 日志中设备重获的重试次数
2. 是否有 "Network route not ready" 多次出现

**解决：**
- 可能是网络环境特殊，考虑增加路由等待次数
- 检查 DNS 设置

### Q3: CPU 使用率高
**检查：**
1. 是否在安装界面停留过久
2. 主程序是否频繁重启

**解决：**
- 安装完成后关闭安装程序
- 检查核心进程是否正常运行

---

## 📊 性能基准

### 重连时间（睡眠唤醒）
- **目标：** < 10 秒
- **优秀：** < 5 秒
- **可接受：** < 15 秒
- **需优化：** > 15 秒

### CPU 使用率
- **空闲：** < 0.5%
- **连接中：** < 2%
- **安装中：** < 1%

### 内存使用
- **主程序：** < 50 MB
- **安装程序：** < 30 MB

---

## 🔄 回滚步骤

如果 0.3.5 出现严重问题：

### 方案 1：完全回滚到 0.3.4
```powershell
git checkout 7130d73  # 0.3.4 的提交
```

### 方案 2：只保留关键修复
保留文件：
- `socket.rs` - Socket 超时
- `main.rs` - 指数退避

回滚文件：
- `udp.rs` - RwLock 修改
- `device.rs` - 发送超时
- `util.rs` - 自旋策略

---

## 📝 版本发布检查清单

### 代码层面
- [ ] 所有测试通过
- [ ] 无编译警告
- [ ] 日志级别正确（Release 应为 Info）

### 文档层面
- [ ] 更新 CHANGELOG.md
- [ ] 更新版本号
- [ ] 更新 README.md（如有新功能）

### 构建层面
- [ ] Release 编译优化开启
- [ ] 签名证书有效（如有）
- [ ] 安装包测试通过

### 发布层面
- [ ] GitHub Release 创建
- [ ] 上传安装包和便携版
- [ ] 标注 Pre-release（如为测试版）
- [ ] 编写 Release Notes

---

## 🎓 代码维护建议

### 新增功能时
1. **优先考虑错误处理**
   - 所有网络操作都应有超时
   - 区分临时错误和永久错误

2. **避免阻塞操作**
   - 不要在锁内执行耗时操作
   - 使用带超时的锁获取

3. **添加日志**
   - 关键操作记录 Info 日志
   - 错误记录 Error 日志并包含上下文

### 修复 Bug 时
1. **先写测试**
   - 复现 Bug 的测试用例
   - 确保修复后测试通过

2. **考虑边界情况**
   - 睡眠唤醒
   - 网络中断
   - 高负载

3. **保持向后兼容**
   - 配置文件格式
   - 日志路径
   - 注册表项

---

## 🔗 相关资源

### 项目链接
- **GitHub：** https://github.com/LuoMuQAQ/drcom4scut
- **最新 Release：** https://github.com/LuoMuQAQ/drcom4scut/releases/tag/windows-v0.3.4

### 参考文档
- `handoff/2026-09-23.md` - 0.3.4 交接文档
- `OPTIMIZATION_SUMMARY.md` - 本次优化总结
- `STABILITY_ANALYSIS.md` - 深度技术分析

### 依赖文档
- Rust 官方文档：https://doc.rust-lang.org/
- Windows API：https://learn.microsoft.com/en-us/windows/win32/
- pnet 库：https://docs.rs/pnet/

---

## ⚡ 快速命令

### 查看日志
```powershell
# 安装版
Get-Content "$env:LOCALAPPDATA\drcom4scutGUI\logs\latest.log" -Tail 50 -Wait

# 便携版
Get-Content ".\logs\latest.log" -Tail 50 -Wait
```

### 清理构建
```powershell
# 核心
cd drcom4scut_0.3.1/vendor/drcom4scut-0.3.2
cargo clean

# GUI
cd ../../../drcom4scut-rs
cargo clean
```

### 运行测试
```powershell
# 核心测试
cd drcom4scut_0.3.1/vendor/drcom4scut-0.3.2
cargo test -- --nocapture

# GUI 测试
cd ../../../drcom4scut-rs
cargo test -- --nocapture
```

---

**文档版本：** 1.0  
**适用版本：** drcom4scut 0.3.5  
**最后更新：** 2026-09-23
