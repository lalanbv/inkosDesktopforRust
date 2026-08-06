import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface LoggingConfig {
  level: string;
  max_file_size_mb: number;
  max_backups: number;
}

export interface UpdatesConfig {
  channel: string;
  auto_check: boolean;
}

export interface EngineConfig {
  startup_timeout_secs: number;
}

export interface NetworkConfig {
  connect_timeout_secs: number;
  request_timeout_secs: number;
}

export interface AppConfig {
  logging: LoggingConfig;
  updates: UpdatesConfig;
  engine: EngineConfig;
  network: NetworkConfig;
}

export type ConfigLayer = 'System' | 'Workspace' | 'Project';

interface ConfigState {
  config: AppConfig | null;
  currentLayer: ConfigLayer;
  loading: boolean;
  error: string | null;
  validationErrors: Record<string, string>;
  hotReloadEnabled: boolean;

  // Actions
  fetchConfig: () => Promise<void>;
  updateConfig: (layer: ConfigLayer, config: AppConfig) => Promise<void>;
  resetConfig: (layer: ConfigLayer) => Promise<void>;
  setCurrentLayer: (layer: ConfigLayer) => void;
  validateField: (field: string, value: any) => string | null;
  enableHotReload: () => Promise<void>;
  disableHotReload: () => Promise<void>;
}

const VALID_LOG_LEVELS = ['trace', 'debug', 'info', 'warn', 'error'];
const VALID_UPDATE_CHANNELS = ['stable', 'beta', 'dev'];

export const useConfigStore = create<ConfigState>((set, get) => ({
  config: null,
  currentLayer: 'System',
  loading: false,
  error: null,
  validationErrors: {},
  hotReloadEnabled: false,

  fetchConfig: async () => {
    set({ loading: true, error: null });
    try {
      const config = await invoke<AppConfig>('get_config');
      set({ config, loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },

  updateConfig: async (layer: ConfigLayer, config: AppConfig) => {
    set({ loading: true, error: null });
    try {
      await invoke('update_config', { layer, config });

      // 重新获取合并后的配置
      await get().fetchConfig();

      set({ loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  resetConfig: async (layer: ConfigLayer) => {
    set({ loading: true, error: null });
    try {
      await invoke('reset_config', { layer });

      // 重新获取配置
      await get().fetchConfig();

      set({ loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
      throw error;
    }
  },

  setCurrentLayer: (layer: ConfigLayer) => {
    set({ currentLayer: layer, validationErrors: {} });
  },

  validateField: (field: string, value: any): string | null => {
    switch (field) {
      case 'logging.level':
        if (!VALID_LOG_LEVELS.includes(value)) {
          return `日志级别必须是: ${VALID_LOG_LEVELS.join(', ')}`;
        }
        break;

      case 'logging.max_file_size_mb':
        if (typeof value !== 'number' || value < 1 || value > 100) {
          return '文件大小必须在 1-100 MB 之间';
        }
        break;

      case 'logging.max_backups':
        if (typeof value !== 'number' || value < 1 || value > 20) {
          return '备份数量必须在 1-20 之间';
        }
        break;

      case 'updates.channel':
        if (!VALID_UPDATE_CHANNELS.includes(value)) {
          return `更新通道必须是: ${VALID_UPDATE_CHANNELS.join(', ')}`;
        }
        break;

      case 'engine.startup_timeout_secs':
        if (typeof value !== 'number' || value < 5 || value > 300) {
          return '启动超时必须在 5-300 秒之间';
        }
        break;

      case 'network.connect_timeout_secs':
        if (typeof value !== 'number' || value < 1 || value > 60) {
          return '连接超时必须在 1-60 秒之间';
        }
        break;

      case 'network.request_timeout_secs':
        if (typeof value !== 'number' || value < 5 || value > 300) {
          return '请求超时必须在 5-300 秒之间';
        }
        break;

      default:
        return null;
    }

    return null;
  },

  enableHotReload: async () => {
    try {
      await invoke('start_config_watch');
      set({ hotReloadEnabled: true });

      // 监听配置变更事件
      const unlisten = await listen<any>('config-changed', (event) => {
        const { config } = event.payload;
        set({ config });
        console.log('配置已热重载:', event.payload);
      });

      // 保存 unlisten 函数以便清理
      (window as any).__configUnlisten = unlisten;
    } catch (error) {
      console.error('启动配置热重载失败:', error);
      throw error;
    }
  },

  disableHotReload: async () => {
    try {
      await invoke('stop_config_watch');
      set({ hotReloadEnabled: false });

      // 清理监听器
      if ((window as any).__configUnlisten) {
        (window as any).__configUnlisten();
        delete (window as any).__configUnlisten;
      }
    } catch (error) {
      console.error('停止配置热重载失败:', error);
      throw error;
    }
  },
}));
