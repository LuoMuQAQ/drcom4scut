import React from 'react';
import { Button } from '@heroui/react';
import { Minus, X, Sun, Moon } from 'lucide-react';

interface TitleBarProps {
  title: string;
  icon?: React.ReactNode;
  theme: 'light' | 'dark';
  themeMode: 'system' | 'light' | 'dark';
  onThemeToggle: () => void;
  onMinimize?: () => void;
  onClose?: () => void;
  showThemeToggle?: boolean;
}

export const TitleBar: React.FC<TitleBarProps> = ({
  title,
  icon,
  theme,
  themeMode,
  onThemeToggle,
  onMinimize,
  onClose,
  showThemeToggle = true,
}) => {
  return (
    <div className="titlebar-drag-region h-10 w-full flex items-center justify-between px-3 border-b border-zinc-200 dark:border-zinc-800/80 bg-zinc-50/80 dark:bg-zinc-950/80 backdrop-blur-md select-none shrink-0">
      <div className="flex items-center gap-2">
        {icon}
        <span className="text-xs font-semibold text-zinc-800 dark:text-zinc-200 tracking-wide">
          {title}
        </span>
      </div>

      <div className="no-drag flex items-center gap-0.5">
        {showThemeToggle && (
          <Button
            size="sm"
            variant="ghost"
            className="w-7 h-7 min-w-0 p-0 rounded-full text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
            title={`当前主题模式: ${themeMode}`}
            onPress={onThemeToggle}
          >
            {theme === 'dark' ? (
              <Sun className="w-3.5 h-3.5 text-amber-400" />
            ) : (
              <Moon className="w-3.5 h-3.5 text-zinc-600" />
            )}
          </Button>
        )}

        {onMinimize && (
          <Button
            size="sm"
            variant="ghost"
            className="w-7 h-7 min-w-0 p-0 rounded-md text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200 hover:bg-zinc-200/50 dark:hover:bg-zinc-800/50"
            onPress={onMinimize}
          >
            <Minus className="w-3.5 h-3.5" />
          </Button>
        )}

        {onClose && (
          <Button
            size="sm"
            variant="ghost"
            className="w-7 h-7 min-w-0 p-0 rounded-md text-zinc-500 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-500/10 dark:hover:bg-red-500/20"
            onPress={onClose}
          >
            <X className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    </div>
  );
};
