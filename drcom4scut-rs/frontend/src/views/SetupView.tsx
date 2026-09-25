import React, { useState, useEffect } from 'react';
import { Button } from '@heroui/react';
import {
  DownloadCloud,
  Folder,
  CheckCircle2,
  AlertTriangle,
  RotateCw,
  ExternalLink,
  ShieldCheck,
  XCircle,
} from 'lucide-react';
import { TitleBar } from '../components/TitleBar';
import { ipc, InitSnapshot } from '../services/ipc';

interface SetupViewProps {
  data: NonNullable<InitSnapshot['setup']>;
  theme: 'light' | 'dark';
  themeMode: 'system' | 'light' | 'dark';
  onThemeToggle: () => void;
}

export const SetupView: React.FC<SetupViewProps> = ({
  data,
  theme,
  themeMode,
  onThemeToggle,
}) => {
  const [page, setPage] = useState<'configure' | 'running' | 'finished'>('configure');
  const [installDir, setInstallDir] = useState(data.defaultDir);
  const [dirValid, setDirValid] = useState(true);
  const [dirError, setDirError] = useState<string | null>(null);

  const [desktopShortcut, setDesktopShortcut] = useState(data.desktopShortcut);
  const [startMenuShortcut, setStartMenuShortcut] = useState(data.startMenuShortcut);
  const [launchAfter, setLaunchAfter] = useState(data.launchAfter);

  const [progress, setProgress] = useState(0);
  const [progressMsg, setProgressMsg] = useState('准备中…');

  const [resultOk, setResultOk] = useState(true);
  const [resultMsg, setResultMsg] = useState('');
  const [needReboot, setNeedReboot] = useState(false);

  useEffect(() => {
    const unsubscribe = ipc.subscribe((event) => {
      if (event.type === 'installDirValidated') {
        setInstallDir(event.payload.path);
        setDirValid(event.payload.valid);
        setDirError(event.payload.error || null);
      } else if (event.type === 'installProgress') {
        setPage('running');
        setProgress(event.payload.percent);
        setProgressMsg(event.payload.message);
      } else if (event.type === 'installFinished') {
        setPage('finished');
        setResultOk(event.payload.ok);
        setResultMsg(event.payload.message);
        setNeedReboot(event.payload.needReboot);
      }
    });
    return unsubscribe;
  }, []);

  const handleStartInstall = () => {
    ipc.startInstall();
  };

  const handleCancel = () => {
    if (page === 'running') {
      ipc.cancelInstall();
    } else {
      ipc.windowAction('close');
    }
  };

  return (
    <div className="flex flex-col h-full w-full bg-zinc-50 dark:bg-zinc-950 text-zinc-900 dark:text-zinc-100 select-none overflow-hidden">
      <TitleBar
        title="校园网客户端 · 安装向导"
        icon={<DownloadCloud className="w-4 h-4 text-blue-500" />}
        theme={theme}
        themeMode={themeMode}
        onThemeToggle={onThemeToggle}
        onClose={handleCancel}
      />

      <div className="flex-1 p-6 flex flex-col justify-between overflow-y-auto">
        {page === 'configure' && (
          <div className="flex flex-col gap-4">
            {/* 顶栏说明 */}
            <div className="flex items-center gap-3">
              <div className="p-2.5 rounded-2xl bg-blue-500/10 text-blue-600 dark:text-blue-400">
                <ShieldCheck className="w-6 h-6" />
              </div>
              <div>
                <h2 className="text-base font-bold text-zinc-900 dark:text-zinc-100">安装校园网认证客户端</h2>
                <p className="text-xs text-zinc-500 dark:text-zinc-400">高效稳定、自适应网卡与安全守护</p>
              </div>
            </div>

            {/* 安装目录配置 */}
            <div className="p-3.5 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex flex-col gap-2">
              <label className="text-xs font-semibold text-zinc-700 dark:text-zinc-300">安装位置</label>
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={installDir}
                  onChange={(e) => {
                    setInstallDir(e.target.value);
                    ipc.setInstallDir(e.target.value);
                  }}
                  className={`flex-1 px-3 py-2 text-xs rounded-xl bg-zinc-100 dark:bg-zinc-800 border focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono ${
                    dirValid ? 'border-zinc-200 dark:border-zinc-700' : 'border-red-500 text-red-500'
                  }`}
                />
                <Button
                  size="sm"
                  variant="flat"
                  className="bg-zinc-200 dark:bg-zinc-800 text-xs px-3 rounded-xl font-medium"
                  startContent={<Folder className="w-3.5 h-3.5" />}
                  onPress={() => ipc.browseInstallDir()}
                >
                  浏览…
                </Button>
              </div>
              {dirError && <span className="text-[11px] text-red-500">{dirError}</span>}
            </div>

            {/* Npcap 驱动状态卡片 */}
            <div className="p-3 rounded-2xl bg-zinc-100 dark:bg-zinc-900/60 border border-zinc-200/80 dark:border-zinc-800 flex items-center justify-between">
              <div className="flex items-center gap-2 text-xs">
                <span className="font-semibold">底层网络驱动:</span>
                <span className="text-zinc-500">{data.driverDetail || 'Npcap 抓包引擎'}</span>
              </div>
              <span className="text-[11px] px-2 py-0.5 rounded-full bg-blue-500/10 text-blue-600 dark:text-blue-400 font-medium">
                {data.driverStatus || '自动配置'}
              </span>
            </div>

            {/* 选项复选框 */}
            <div className="p-3 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 flex flex-col gap-2 text-xs">
              <label className="flex items-center justify-between cursor-pointer">
                <span>创建桌面快捷方式</span>
                <input
                  type="checkbox"
                  checked={desktopShortcut}
                  onChange={(e) => {
                    setDesktopShortcut(e.target.checked);
                    ipc.setInstallOption('desktopShortcut', e.target.checked);
                  }}
                  className="w-4 h-4 rounded text-blue-600"
                />
              </label>
              <label className="flex items-center justify-between cursor-pointer">
                <span>添加到开始菜单</span>
                <input
                  type="checkbox"
                  checked={startMenuShortcut}
                  onChange={(e) => {
                    setStartMenuShortcut(e.target.checked);
                    ipc.setInstallOption('startMenuShortcut', e.target.checked);
                  }}
                  className="w-4 h-4 rounded text-blue-600"
                />
              </label>
              <label className="flex items-center justify-between cursor-pointer">
                <span>安装完成后启动客户端</span>
                <input
                  type="checkbox"
                  checked={launchAfter}
                  onChange={(e) => {
                    setLaunchAfter(e.target.checked);
                    ipc.setInstallOption('launchAfter', e.target.checked);
                  }}
                  className="w-4 h-4 rounded text-blue-600"
                />
              </label>
            </div>
          </div>
        )}

        {page === 'running' && (
          <div className="flex-1 flex flex-col justify-center items-center text-center gap-4 py-8">
            <div className="p-3 rounded-2xl bg-blue-500/10 text-blue-600 animate-spin">
              <RotateCw className="w-8 h-8" />
            </div>
            <div className="flex flex-col gap-1 w-full max-w-sm">
              <h3 className="text-sm font-bold">正在安装校园网认证客户端…</h3>
              <p className="text-xs text-zinc-500 line-clamp-1">{progressMsg}</p>
            </div>
            {/* 进度条 */}
            <div className="w-full max-w-sm h-2.5 rounded-full bg-zinc-200 dark:bg-zinc-800 overflow-hidden">
              <div
                className="h-full bg-blue-600 rounded-full transition-all duration-300"
                style={{ width: `${progress}%` }}
              />
            </div>
            <span className="text-xs font-mono font-medium text-zinc-500">{progress}%</span>
          </div>
        )}

        {page === 'finished' && (
          <div className="flex-1 flex flex-col justify-center items-center text-center gap-4 py-8">
            <div
              className={`p-3 rounded-2xl ${
                resultOk ? 'bg-emerald-500/10 text-emerald-500' : 'bg-rose-500/10 text-rose-500'
              }`}
            >
              {resultOk ? <CheckCircle2 className="w-10 h-10" /> : <XCircle className="w-10 h-10" />}
            </div>
            <div className="flex flex-col gap-1 max-w-sm">
              <h3 className="text-base font-bold">
                {resultOk ? '校园网客户端安装完成' : '安装未能完成'}
              </h3>
              <p className="text-xs text-zinc-500 dark:text-zinc-400">
                {resultMsg || (resultOk ? '所有核心组件已准备就绪。' : '请检查错误日志或权限。')}
              </p>
              {needReboot && (
                <div className="mt-2 p-2 rounded-xl bg-amber-500/10 text-amber-600 text-xs flex items-center justify-center gap-1.5">
                  <AlertTriangle className="w-3.5 h-3.5" /> 驱动安装需要重启计算机后生效
                </div>
              )}
            </div>
          </div>
        )}

        {/* 底部按钮栏 */}
        <div className="pt-4 border-t border-zinc-200 dark:border-zinc-800 flex items-center justify-end gap-2">
          {page === 'configure' && (
            <>
              <Button
                variant="light"
                size="sm"
                className="text-xs rounded-xl"
                onPress={() => ipc.windowAction('close')}
              >
                取消
              </Button>
              <Button
                color="primary"
                size="sm"
                className="text-xs font-bold bg-blue-600 hover:bg-blue-500 text-white rounded-xl px-5"
                isDisabled={!dirValid}
                onPress={handleStartInstall}
              >
                立即安装
              </Button>
            </>
          )}

          {page === 'running' && (
            <Button
              variant="flat"
              color="danger"
              size="sm"
              className="text-xs rounded-xl text-rose-600 bg-rose-500/10"
              onPress={handleCancel}
            >
              取消安装
            </Button>
          )}

          {page === 'finished' && (
            <Button
              color="primary"
              size="sm"
              className="text-xs font-bold bg-blue-600 hover:bg-blue-500 text-white rounded-xl px-6"
              onPress={() => ipc.windowAction('close')}
            >
              完成
            </Button>
          )}
        </div>
      </div>
    </div>
  );
};
