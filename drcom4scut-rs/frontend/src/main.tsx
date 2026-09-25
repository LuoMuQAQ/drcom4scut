import React, { useState, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { Button } from '@heroui/react';
import { Sun, Moon, Send, Activity, ShieldCheck, CheckCircle2 } from 'lucide-react';
import './index.css';

// 宿主 IPC 接口定义
declare global {
  interface Window {
    chrome?: {
      webview?: {
        postMessage: (message: any) => void;
        addEventListener: (type: string, listener: (event: any) => void) => void;
        removeEventListener: (type: string, listener: (event: any) => void) => void;
      };
    };
  }
}

function PrototypeApp() {
  const [theme, setTheme] = useState<'light' | 'dark'>('dark');
  const [pingCount, setPingCount] = useState<number>(0);
  const [rustResponse, setRustResponse] = useState<string>('等待 Rust 发送状态...');
  const [connectedTime, setConnectedTime] = useState<string>('');

  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'dark') {
      root.classList.add('dark');
      root.classList.remove('light');
    } else {
      root.classList.add('light');
      root.classList.remove('dark');
    }
  }, [theme]);

  useEffect(() => {
    const handleMessage = (event: any) => {
      console.log('Received message from Rust:', event.data);
      if (typeof event.data === 'string') {
        try {
          const parsed = JSON.parse(event.data);
          if (parsed.type === 'PONG') {
            setRustResponse(`[Rust 确认] 收到 Ping #${parsed.count}, 时间: ${parsed.timestamp}`);
          } else if (parsed.type === 'STATUS') {
            setRustResponse(`[Rust 状态] ${parsed.status}`);
            setConnectedTime(parsed.time || '');
          }
        } catch {
          setRustResponse(`[Rust 消息] ${event.data}`);
        }
      } else if (typeof event.data === 'object') {
        setRustResponse(`[Rust 对象] ${JSON.stringify(event.data)}`);
      }
    };

    if (window.chrome?.webview) {
      window.chrome.webview.addEventListener('message', handleMessage);
      window.chrome.webview.postMessage({ type: 'READY', client: 'HeroUI-v3-Prototype' });
    }

    return () => {
      if (window.chrome?.webview) {
        window.chrome.webview.removeEventListener('message', handleMessage);
      }
    };
  }, []);

  const sendPingToRust = () => {
    const nextCount = pingCount + 1;
    setPingCount(nextCount);
    if (window.chrome?.webview) {
      window.chrome.webview.postMessage({
        type: 'PING',
        count: nextCount,
        timestamp: new Date().toLocaleTimeString(),
      });
    } else {
      setRustResponse('当前未运行在 WebView2 环境下（无法发送 postMessage）');
    }
  };

  const toggleTheme = () => {
    const nextTheme = theme === 'dark' ? 'light' : 'dark';
    setTheme(nextTheme);
    if (window.chrome?.webview) {
      window.chrome.webview.postMessage({
        type: 'THEME_CHANGE',
        theme: nextTheme,
      });
    }
  };

  return (
    <div className={`flex flex-col h-full w-full p-4 transition-colors duration-200 ${
      theme === 'dark' ? 'bg-zinc-950 text-zinc-100' : 'bg-zinc-50 text-zinc-900'
    }`}>
      {/* 标题栏 */}
      <div className="titlebar-drag-region flex items-center justify-between pb-3 border-b border-zinc-200 dark:border-zinc-800">
        <div className="flex items-center gap-2">
          <Activity className="w-5 h-5 text-blue-500 animate-pulse" />
          <span className="font-semibold text-sm tracking-wide">DrCOM 校园网客户端 · 原型</span>
        </div>
        <div className="no-drag flex items-center gap-1">
          <Button
            size="sm"
            variant="ghost"
            className="w-8 h-8 p-0 rounded-full"
            onPress={toggleTheme}
          >
            {theme === 'dark' ? <Sun className="w-4 h-4 text-amber-400" /> : <Moon className="w-4 h-4 text-zinc-600" />}
          </Button>
        </div>
      </div>

      {/* 主体内容 */}
      <div className="flex-1 flex flex-col justify-center items-center gap-4 py-4 text-center">
        <div className="p-3 rounded-2xl bg-blue-500/10 text-blue-500 border border-blue-500/20">
          <ShieldCheck className="w-10 h-10" />
        </div>

        <div className="flex flex-col gap-1">
          <h2 className="text-base font-bold">HeroUI v3 + WebView2 通信可行性验证</h2>
          <p className="text-xs text-zinc-500 dark:text-zinc-400 max-w-xs">
            此原型正在验证 React 19 + HeroUI 3.x 界面渲染、离线静态资源打包、双向安全命令事件回传。
          </p>
        </div>

        {/* 状态卡片 */}
        <div className="w-full max-w-xs p-3 rounded-xl bg-zinc-100 dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex flex-col gap-2 text-left">
          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-500 dark:text-zinc-400">当前模式:</span>
            <span className="font-mono font-medium capitalize text-blue-500">{theme} 主题</span>
          </div>
          <div className="flex items-center justify-between text-xs">
            <span className="text-zinc-500 dark:text-zinc-400">已发 Ping:</span>
            <span className="font-mono font-medium">{pingCount} 次</span>
          </div>
          <div className="pt-2 border-t border-zinc-200 dark:border-zinc-800">
            <div className="text-[11px] text-zinc-400 dark:text-zinc-500 mb-1">来自 Rust 的回执:</div>
            <div className="text-xs font-mono bg-zinc-200/50 dark:bg-zinc-800/50 p-2 rounded text-zinc-700 dark:text-zinc-300 break-all">
              {rustResponse}
            </div>
          </div>
        </div>

        {/* 交互按钮 */}
        <div className="flex gap-2 w-full max-w-xs justify-center">
          <Button
            color="primary"
            className="flex-1 font-medium text-xs bg-blue-600 hover:bg-blue-500 text-white rounded-lg shadow-sm"
            startContent={<Send className="w-3.5 h-3.5" />}
            onPress={sendPingToRust}
          >
            发送受控 Ping 给 Rust
          </Button>
        </div>
      </div>

      {/* 底部信息 */}
      <div className="text-[11px] text-zinc-400 dark:text-zinc-500 flex items-center justify-between pt-2 border-t border-zinc-200 dark:border-zinc-800">
        <span>静态离线资源（无外部 CDN）</span>
        <span className="flex items-center gap-1 text-emerald-500">
          <CheckCircle2 className="w-3 h-3" /> 边界受控
        </span>
      </div>
    </div>
  );
}

const root = ReactDOM.createRoot(document.getElementById('root')!);
root.render(
  <React.StrictMode>
    <PrototypeApp />
  </React.StrictMode>
);
