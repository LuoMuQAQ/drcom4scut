import React, { useState, useEffect } from 'react';
import { ipc, InitSnapshot, ThemeMode } from './services/ipc';
import { MainView } from './views/MainView';
import { SetupView } from './views/SetupView';
import { UninstallView } from './views/UninstallView';
import { RotateCw } from 'lucide-react';

export const App: React.FC = () => {
  const [snapshot, setSnapshot] = useState<InitSnapshot | null>(null);
  const [themeMode, setThemeMode] = useState<ThemeMode>('system');
  const [actualTheme, setActualTheme] = useState<'light' | 'dark'>('dark');

  useEffect(() => {
    // 订阅 Rust 事件
    const unsubscribe = ipc.subscribe((event) => {
      if (event.type === 'initSnapshot') {
        setSnapshot(event.payload);
        setThemeMode(event.payload.themeMode);
        setActualTheme(event.payload.actualTheme);
      } else if (event.type === 'themeChanged') {
        setThemeMode(event.payload.mode);
        setActualTheme(event.payload.actual);
      }
    });

    // 页面就绪后请求初始化快照
    ipc.requestInit();

    return unsubscribe;
  }, []);

  useEffect(() => {
    // 动态同步 html 根标签主题类
    const root = document.documentElement;
    if (actualTheme === 'dark') {
      root.classList.add('dark');
      root.classList.remove('light');
    } else {
      root.classList.add('light');
      root.classList.remove('dark');
    }
  }, [actualTheme]);

  const handleThemeToggle = () => {
    let nextMode: ThemeMode = 'dark';
    if (themeMode === 'system') {
      nextMode = actualTheme === 'dark' ? 'light' : 'dark';
    } else if (themeMode === 'light') {
      nextMode = 'dark';
    } else if (themeMode === 'dark') {
      nextMode = 'system';
    }
    setThemeMode(nextMode);
    ipc.setTheme(nextMode);
  };

  if (!snapshot) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-zinc-950 text-zinc-100">
        <div className="flex flex-col items-center gap-2">
          <RotateCw className="w-6 h-6 animate-spin text-blue-500" />
          <span className="text-xs text-zinc-400 font-mono">正在加载校园网客户端…</span>
        </div>
      </div>
    );
  }

  if (snapshot.windowType === 'setup' && snapshot.setup) {
    return (
      <SetupView
        data={snapshot.setup}
        theme={actualTheme}
        themeMode={themeMode}
        onThemeToggle={handleThemeToggle}
      />
    );
  }

  if (snapshot.windowType === 'uninstall' && snapshot.uninstall) {
    return (
      <UninstallView
        data={snapshot.uninstall}
        theme={actualTheme}
        themeMode={themeMode}
        onThemeToggle={handleThemeToggle}
      />
    );
  }

  // 默认渲染主窗口
  const defaultMain = snapshot.main || {
    settings: {
      username: '',
      hasPassword: false,
      autoConnect: false,
      rememberPassword: true,
      startup: false,
      trayKeep: true,
    },
    adapters: [],
    status: {
      state: 'idle',
      title: '未连接',
      detail: '点击连接按钮发起认证',
      indicator: 'idle',
    },
  };

  return (
    <MainView
      data={defaultMain}
      theme={actualTheme}
      themeMode={themeMode}
      onThemeToggle={handleThemeToggle}
    />
  );
};
