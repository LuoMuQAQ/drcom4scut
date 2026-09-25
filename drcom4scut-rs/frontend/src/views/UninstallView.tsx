import React, { useState, useEffect } from 'react';
import { Button } from '@heroui/react';
import { Trash2, AlertTriangle, CheckCircle2, RotateCw } from 'lucide-react';
import { TitleBar } from '../components/TitleBar';
import { ipc, InitSnapshot } from '../services/ipc';

interface UninstallViewProps {
  data: NonNullable<InitSnapshot['uninstall']>;
  theme: 'light' | 'dark';
  themeMode: 'system' | 'light' | 'dark';
  onThemeToggle: () => void;
}

export const UninstallView: React.FC<UninstallViewProps> = ({
  data,
  theme,
  themeMode,
  onThemeToggle,
}) => {
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState(false);
  const [resultMsg, setResultMsg] = useState('');

  useEffect(() => {
    const unsubscribe = ipc.subscribe((event) => {
      if (event.type === 'uninstallFinished') {
        setRunning(false);
        setFinished(true);
        setResultMsg(event.payload.message);
      }
    });
    return unsubscribe;
  }, []);

  const handleConfirm = () => {
    setRunning(true);
    ipc.confirmUninstall();
  };

  const handleCancel = () => {
    ipc.cancelUninstall();
  };

  return (
    <div className="flex flex-col h-full w-full bg-zinc-50 dark:bg-zinc-950 text-zinc-900 dark:text-zinc-100 select-none overflow-hidden">
      <TitleBar
        title="卸载校园网认证客户端"
        icon={<Trash2 className="w-4 h-4 text-rose-500" />}
        theme={theme}
        themeMode={themeMode}
        onThemeToggle={onThemeToggle}
        onClose={handleCancel}
      />

      <div className="flex-1 p-6 flex flex-col justify-between">
        {!finished ? (
          <div className="flex flex-col gap-4">
            <div className="flex items-center gap-3">
              <div className="p-3 rounded-2xl bg-rose-500/10 text-rose-600 dark:text-rose-400">
                <AlertTriangle className="w-8 h-8" />
              </div>
              <div>
                <h2 className="text-base font-bold text-zinc-900 dark:text-zinc-100">确定卸载校园网认证客户端？</h2>
                <p className="text-xs text-zinc-500 dark:text-zinc-400">版本 {data.version || '0.3.6'}</p>
              </div>
            </div>

            {/* 警告详情卡片 */}
            <div className="p-4 rounded-2xl bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-800 text-xs flex flex-col gap-2">
              <span className="font-semibold text-zinc-700 dark:text-zinc-300">将删除以下内容：</span>
              <ul className="list-disc list-inside text-zinc-500 dark:text-zinc-400 space-y-1">
                <li>安装目录内所有文件（{data.installDir}）</li>
                <li>已保存的账号设置、本地配置文件及连接日志</li>
                <li>桌面及开始菜单快捷方式、系统卸载条目</li>
              </ul>
              <div className="pt-2 mt-1 border-t border-zinc-100 dark:border-zinc-800 text-[11px] text-zinc-400">
                注意：系统中的 Npcap 驱动不会被移除。此操作不可撤销。
              </div>
            </div>
          </div>
        ) : (
          <div className="flex-1 flex flex-col justify-center items-center text-center gap-4 py-8">
            <div className="p-3 rounded-2xl bg-emerald-500/10 text-emerald-500">
              <CheckCircle2 className="w-10 h-10" />
            </div>
            <div className="flex flex-col gap-1 max-w-xs">
              <h3 className="text-base font-bold">卸载完成</h3>
              <p className="text-xs text-zinc-500 dark:text-zinc-400">{resultMsg || '相关文件与快捷方式已被清除。'}</p>
            </div>
          </div>
        )}

        {/* 底部按钮 */}
        <div className="pt-4 border-t border-zinc-200 dark:border-zinc-800 flex items-center justify-end gap-2">
          {!finished ? (
            <>
              <Button
                variant="light"
                size="sm"
                className="text-xs rounded-xl"
                isDisabled={running}
                onPress={handleCancel}
              >
                取消
              </Button>
              <Button
                color="danger"
                size="sm"
                className="text-xs font-bold bg-rose-600 hover:bg-rose-500 text-white rounded-xl px-5"
                isLoading={running}
                onPress={handleConfirm}
              >
                确认卸载
              </Button>
            </>
          ) : (
            <Button
              color="primary"
              size="sm"
              className="text-xs font-bold bg-blue-600 hover:bg-blue-500 text-white rounded-xl px-6"
              onPress={() => ipc.windowAction('close')}
            >
              关闭
            </Button>
          )}
        </div>
      </div>
    </div>
  );
};
