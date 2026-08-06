import { useEffect } from 'react';
import { useConfigStore, ConfigLayer } from '../stores/config';

export function useConfig() {
  const {
    config,
    currentLayer,
    loading,
    error,
    validationErrors,
    hotReloadEnabled,
    fetchConfig,
    updateConfig,
    resetConfig,
    setCurrentLayer,
    validateField,
    enableHotReload,
    disableHotReload,
  } = useConfigStore();

  // 自动加载配置
  useEffect(() => {
    fetchConfig();
  }, [fetchConfig]);

  // 自动启动热重载
  useEffect(() => {
    if (!hotReloadEnabled) {
      enableHotReload().catch((err) => {
        console.error('自动启动热重载失败:', err);
      });
    }

    return () => {
      if (hotReloadEnabled) {
        disableHotReload().catch((err) => {
          console.error('清理热重载失败:', err);
        });
      }
    };
  }, []);

  return {
    config,
    currentLayer,
    loading,
    error,
    validationErrors,
    hotReloadEnabled,
    updateConfig,
    resetConfig,
    setCurrentLayer,
    validateField,
    refetch: fetchConfig,
    enableHotReload,
    disableHotReload,
  };
}
