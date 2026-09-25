import React, { useState, useEffect } from 'react';
import { Button, Input, Switch, Select } from '@heroui/react';
import {
  Wifi,
  WifiOff,
  User,
  Lock,
  Eye,
  EyeOff,
  Network,
  Power,
  RotateCw,
  CheckCircle2,
  AlertCircle,
  Clock,
} from 'lucide-react';
import { TitleBar } from '../components/TitleBar';
import { ipc, InitSnapshot } from '../services/ipc';

interface MainViewProps {
  data: NonNullable<InitSnapshot['main']>;
  theme: 'light' | 'dark';
  themeMode: 'system' | 'light' | 'dark';
  onThemeToggle: () => void;
}

export const MainView: React.FC<MainViewProps> = ({
  data,
  theme,
  themeMode,
  onThemeToggle,
}) => {
  const [username, setUsername] = useState(data.settings.username || '');
  const [password, setPassword] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [hasSavedPassword, setHasSavedPassword] = useState(data.settings.hasPassword);

  const [autoConnect, setAutoConnect] = useState(data.settings.autoConnect);
  const [rememberPassword, setRememberPassword] = useState(data.settings.rememberPassword);
  const [startup, setStartup] = useState(data.settings.startup);
  const [trayKeep, setTrayKeep] = useState(data.settings.trayKeep);

  const [selectedAdapter, setSelectedAdapter] = useState(data.selectedAdapterId || '');
  const [status, setStatus] = useState(data.status);

  useEffect(() => {
    const unsubscribe = ipc.subscribe((event) => {
      if (event.type === 'statusChanged') {
        setStatus(event.payload);
      } else if (event.type === 'settingsUpdated') {
        setUsername(event.payload.username);
        setHasSavedPassword(event.payload.hasPassword);
        setAutoConnect(event.payload.autoConnect);
        setRememberPassword(event.payload.rememberPassword);
        setStartup(event.payload.startup);
        setTrayKeep(event.payload.trayKeep);
      } else if (event.type === 'adaptersUpdated') {
        if (event.payload.selectedId) {
          setSelectedAdapter(event.payload.selectedId);
        }
      }
    });
    return unsubscribe;
  }, []);

  const handleConnectToggle = () => {
    if (status.state === 'connected' || status.state === 'connecting') {
      ipc.disconnect();
    } else {
      // 提交设置及新密码（如果有）
      ipc.updateSettings({
        username,
        password: password.length > 0 ? password : undefined,
        autoConnect,
        rememberPassword,
        startup,
        trayKeep,
      });
      ipc.connect();
    }
  };

  const handleAdapterChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const val = e.target.value;
    setSelectedAdapter(val);
    ipc.selectAdapter(val);
  };

  const isConnecting = status.state === 'connecting';
  const isConnected = status.state === 'connected';

  return (
    <div className="flex flex-col h-full w-full bg-zinc-50 dark:bg-zinc-950 text-zinc-900 dark:text-zinc-100 select-none overflow-hidden">
      {/* 标题栏 */}
      <TitleBar
        title="校园网认证客户端"
        icon={<Wifi className="w-4 h-4 text-blue-500" />}
        theme={theme}
        themeMode={themeMode}
        onThemeToggle={onThemeToggle}
        onMinimize={() => ipc.windowAction('minimize')}
        onClose={() => ipc.windowAction('close')}
      />

      {/* 主体交互区 */}
      <div className="flex-1 p-4 flex flex-col gap-3 overflow-y-auto">
        {/* 1. 状态卡片 */}
        <div className="p-3.5 rounded-2xl bg-white dark:bg-zinc-900/90 border border-zinc-200/80 dark:border-zinc-800 shadow-sm flex items-center justify-between gap-3 transition-all duration-300">
          <div className="flex items-center gap-3">
            {/* 状态徽标与呼吸灯 */}
            <div
              className={`w-10 h-10 rounded-xl flex items-center justify-center transition-all ${
                isConnected
                  ? 'bg-emerald-500/10 text-emerald-500 ring-2 ring-emerald-500/20'
                  : isConnecting
                  ? 'bg-blue-500/10 text-blue-500 animate-pulse ring-2 ring-blue-500/20'
                  : 'bg-zinc-200/60 dark:bg-zinc-800 text-zinc-500'
              }`}
            >
              {isConnected ? (
                <CheckCircle2 className="w-5 h-5" />
              ) : isConnecting ? (
                <RotateCw className="w-5 h-5 animate-spin" />
              ) : (
                <WifiOff className="w-5 h-5" />
              )}
            </div>

            <div className="flex flex-col">
              <span className="text-sm font-bold tracking-tight text-zinc-900 dark:text-zinc-100">
                {status.title}
              </span>
              <span className="text-xs text-zinc-500 dark:text-zinc-400 font-mono line-clamp-1">
                {status.detail}
              </span>
            </div>
          </div>

          <div className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-medium bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 border border-zinc-200/60 dark:border-zinc-700/60">
            <span
              className={`w-2 h-2 rounded-full ${
                isConnected
                  ? 'bg-emerald-500 animate-pulse'
                  : isConnecting
                  ? 'bg-blue-500 animate-ping'
                  : 'bg-zinc-400'
              }`}
            />
            <span>{isConnected ? '在线' : isConnecting ? '连接中' : '离线'}</span>
          </div>
        </div>

        {/* 2. 账号与网络输入卡片 */}
        <div className="p-4 rounded-2xl bg-white dark:bg-zinc-900/90 border border-zinc-200/80 dark:border-zinc-800 shadow-sm flex flex-col gap-3">
          {/* 学号输入框 */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 flex items-center gap-1.5">
              <User className="w-3.5 h-3.5 text-zinc-500" /> 学号 / 账号
            </label>
            <input
              type="text"
              value={username}
              placeholder="请输入学号"
              onChange={(e) => setUsername(e.target.value)}
              className="w-full px-3 py-2 text-xs rounded-xl bg-zinc-100/80 dark:bg-zinc-800/80 border border-zinc-200 dark:border-zinc-700/80 focus:outline-none focus:ring-2 focus:ring-blue-500 transition"
            />
          </div>

          {/* 密码输入框 */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 flex items-center gap-1.5">
              <Lock className="w-3.5 h-3.5 text-zinc-500" /> 密码
            </label>
            <div className="relative flex items-center">
              <input
                type={showPassword ? 'text' : 'password'}
                value={password}
                placeholder={hasSavedPassword ? '••••••••（已保存凭据）' : '请输入密码'}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full px-3 py-2 pr-9 text-xs rounded-xl bg-zinc-100/80 dark:bg-zinc-800/80 border border-zinc-200 dark:border-zinc-700/80 focus:outline-none focus:ring-2 focus:ring-blue-500 transition"
              />
              <button
                type="button"
                className="no-drag absolute right-2.5 p-1 text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200 transition"
                onClick={() => setShowPassword(!showPassword)}
              >
                {showPassword ? <EyeOff className="w-3.5 h-3.5" /> : <Eye className="w-3.5 h-3.5" />}
              </button>
            </div>
          </div>

          {/* 网卡选择下拉菜单 */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-semibold text-zinc-700 dark:text-zinc-300 flex items-center gap-1.5">
              <Network className="w-3.5 h-3.5 text-zinc-500" /> 认证网卡
            </label>
            <select
              value={selectedAdapter}
              onChange={handleAdapterChange}
              className="w-full px-3 py-2 text-xs rounded-xl bg-zinc-100/80 dark:bg-zinc-800/80 border border-zinc-200 dark:border-zinc-700/80 focus:outline-none focus:ring-2 focus:ring-blue-500 transition"
            >
              {data.adapters.map((ad) => (
                <option key={ad.id} value={ad.id}>
                  {ad.name} {ad.hasGateway ? '★ 默认网关' : ''} ({ad.mac})
                </option>
              ))}
            </select>
          </div>

          {/* 四项开关网格 */}
          <div className="grid grid-cols-2 gap-2 pt-2 border-t border-zinc-100 dark:border-zinc-800">
            <label className="flex items-center justify-between p-2 rounded-xl bg-zinc-50 dark:bg-zinc-800/40 text-xs cursor-pointer hover:bg-zinc-100 dark:hover:bg-zinc-800 transition">
              <span className="text-zinc-700 dark:text-zinc-300 text-[11px]">自动连接</span>
              <input
                type="checkbox"
                checked={autoConnect}
                onChange={(e) => {
                  setAutoConnect(e.target.checked);
                  ipc.updateSettings({ autoConnect: e.target.checked });
                }}
                className="w-4 h-4 rounded text-blue-600 focus:ring-blue-500"
              />
            </label>

            <label className="flex items-center justify-between p-2 rounded-xl bg-zinc-50 dark:bg-zinc-800/40 text-xs cursor-pointer hover:bg-zinc-100 dark:hover:bg-zinc-800 transition">
              <span className="text-zinc-700 dark:text-zinc-300 text-[11px]">记住密码</span>
              <input
                type="checkbox"
                checked={rememberPassword}
                onChange={(e) => {
                  setRememberPassword(e.target.checked);
                  ipc.updateSettings({ rememberPassword: e.target.checked });
                }}
                className="w-4 h-4 rounded text-blue-600 focus:ring-blue-500"
              />
            </label>

            <label className="flex items-center justify-between p-2 rounded-xl bg-zinc-50 dark:bg-zinc-800/40 text-xs cursor-pointer hover:bg-zinc-100 dark:hover:bg-zinc-800 transition">
              <span className="text-zinc-700 dark:text-zinc-300 text-[11px]">开机自启</span>
              <input
                type="checkbox"
                checked={startup}
                onChange={(e) => {
                  setStartup(e.target.checked);
                  ipc.updateSettings({ startup: e.target.checked });
                }}
                className="w-4 h-4 rounded text-blue-600 focus:ring-blue-500"
              />
            </label>

            <label className="flex items-center justify-between p-2 rounded-xl bg-zinc-50 dark:bg-zinc-800/40 text-xs cursor-pointer hover:bg-zinc-100 dark:hover:bg-zinc-800 transition">
              <span className="text-zinc-700 dark:text-zinc-300 text-[11px]">最小化托盘</span>
              <input
                type="checkbox"
                checked={trayKeep}
                onChange={(e) => {
                  setTrayKeep(e.target.checked);
                  ipc.updateSettings({ trayKeep: e.target.checked });
                }}
                className="w-4 h-4 rounded text-blue-600 focus:ring-blue-500"
              />
            </label>
          </div>
        </div>

        {/* 3. 主操作大按钮 */}
        <Button
          color={isConnected ? 'danger' : 'primary'}
          size="lg"
          className={`w-full py-3.5 rounded-xl font-bold text-sm tracking-wide shadow-md transition-all active:scale-[0.99] text-white ${
            isConnected
              ? 'bg-rose-600 hover:bg-rose-500 shadow-rose-500/20'
              : isConnecting
              ? 'bg-blue-600/80 cursor-wait'
              : 'bg-blue-600 hover:bg-blue-500 shadow-blue-500/20'
          }`}
          onPress={handleConnectToggle}
          startContent={
            isConnecting ? (
              <RotateCw className="w-4 h-4 animate-spin" />
            ) : isConnected ? (
              <Power className="w-4 h-4" />
            ) : (
              <Wifi className="w-4 h-4" />
            )
          }
        >
          {isConnected ? '断开校园网连接' : isConnecting ? '正在连接认证…' : '立即连接校园网'}
        </Button>
      </div>

      {/* 底部版权与状态 */}
      <div className="h-7 px-4 border-t border-zinc-200/80 dark:border-zinc-800/80 bg-zinc-100/50 dark:bg-zinc-900/50 flex items-center justify-between text-[11px] text-zinc-500 dark:text-zinc-400">
        <span>drcom4scut-rs</span>
        <span className="font-mono">windows-v0.3.6 (HeroUI 3.x)</span>
      </div>
    </div>
  );
};
