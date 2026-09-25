// Rust ↔ 前端强类型 IPC 客户端服务

export type WindowType = 'main' | 'setup' | 'uninstall';
export type ThemeMode = 'system' | 'light' | 'dark';

export interface InitSnapshot {
  protocolVersion: number;
  windowType: WindowType;
  themeMode: ThemeMode;
  actualTheme: 'light' | 'dark';
  main?: {
    settings: {
      username: string;
      hasPassword: boolean;
      autoConnect: boolean;
      rememberPassword: boolean;
      startup: boolean;
      trayKeep: boolean;
    };
    adapters: Array<{
      id: string;
      name: string;
      description: string;
      mac: string;
      isUp: boolean;
      hasGateway: boolean;
    }>;
    selectedAdapterId?: string;
    status: {
      state: string;
      title: string;
      detail: string;
      indicator: string;
    };
  };
  setup?: {
    defaultDir: string;
    desktopShortcut: boolean;
    startMenuShortcut: boolean;
    launchAfter: boolean;
    driverStatus: string;
    driverDetail: string;
  };
  uninstall?: {
    installDir: string;
    version: string;
    isElevated: boolean;
  };
}

export type HostEvent =
  | { type: 'initSnapshot'; payload: InitSnapshot }
  | { type: 'statusChanged'; payload: { state: string; title: string; detail: string; indicator: string } }
  | { type: 'settingsUpdated'; payload: InitSnapshot['main']['settings'] }
  | { type: 'adaptersUpdated'; payload: { adapters: InitSnapshot['main']['adapters']; selectedId?: string } }
  | { type: 'themeChanged'; payload: { mode: ThemeMode; actual: 'light' | 'dark' } }
  | { type: 'installDirValidated'; payload: { path: string; valid: boolean; error?: string } }
  | { type: 'installProgress'; payload: { percent: number; message: string } }
  | { type: 'installFinished'; payload: { ok: boolean; message: string; needReboot: boolean } }
  | { type: 'uninstallFinished'; payload: { ok: boolean; message: string } }
  | { type: 'error'; payload: { code: string; message: string } };

type Listener = (event: HostEvent) => void;

class IPCService {
  private listeners: Set<Listener> = new Set();
  private initialized: boolean = false;

  constructor() {
    if (typeof window !== 'undefined' && window.chrome?.webview) {
      window.chrome.webview.addEventListener('message', (e: any) => {
        try {
          const raw = typeof e.data === 'string' ? JSON.parse(e.data) : e.data;
          this.emit(raw);
        } catch (err) {
          console.error('[IPC] Failed to parse message:', err);
        }
      });
    }
  }

  public subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: HostEvent) {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch (err) {
        console.error('[IPC Listener Error]', err);
      }
    }
  }

  public send(command: string, payload?: any) {
    if (window.chrome?.webview) {
      window.chrome.webview.postMessage({
        type: command,
        payload,
      });
    } else {
      console.warn('[IPC Mock] Send command:', command, payload);
    }
  }

  public requestInit() {
    this.send('init', { clientVersion: '0.3.6' });
  }

  public setTheme(mode: ThemeMode) {
    this.send('setTheme', { mode });
  }

  public windowAction(action: 'minimize' | 'close') {
    this.send('windowAction', { action });
  }

  public connect() {
    this.send('connect');
  }

  public disconnect() {
    this.send('disconnect');
  }

  public updateSettings(settings: {
    username?: string;
    password?: string;
    autoConnect?: boolean;
    rememberPassword?: boolean;
    startup?: boolean;
    trayKeep?: boolean;
  }) {
    this.send('updateSettings', settings);
  }

  public selectAdapter(id: string) {
    this.send('selectAdapter', { id });
  }

  public browseInstallDir() {
    this.send('browseInstallDir');
  }

  public setInstallDir(path: string) {
    this.send('setInstallDir', { path });
  }

  public setInstallOption(key: string, value: boolean) {
    this.send('setInstallOption', { key, value });
  }

  public startInstall() {
    this.send('startInstall');
  }

  public cancelInstall() {
    this.send('cancelInstall');
  }

  public confirmUninstall() {
    this.send('confirmUninstall');
  }

  public cancelUninstall() {
    this.send('cancelUninstall');
  }
}

export const ipc = new IPCService();
