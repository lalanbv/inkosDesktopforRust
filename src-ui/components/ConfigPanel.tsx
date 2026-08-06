import React, { useState, useEffect } from 'react';
import { useConfig } from '../hooks/useConfig';
import { AppConfig, ConfigLayer } from '../stores/config';
import './ConfigPanel.css';

export function ConfigPanel() {
  const {
    config,
    currentLayer,
    loading,
    error,
    validationErrors,
    updateConfig,
    resetConfig,
    setCurrentLayer,
    validateField,
  } = useConfig();

  const [editingConfig, setEditingConfig] = useState<AppConfig | null>(null);
  const [localErrors, setLocalErrors] = useState<Record<string, string>>({});
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [hasChanges, setHasChanges] = useState(false);

  // 初始化编辑配置
  useEffect(() => {
    if (config) {
      setEditingConfig(JSON.parse(JSON.stringify(config)));
    }
  }, [config]);

  // 检测配置变更
  useEffect(() => {
    if (config && editingConfig) {
      setHasChanges(JSON.stringify(config) !== JSON.stringify(editingConfig));
    }
  }, [config, editingConfig]);

  const handleFieldChange = (field: string, value: any) => {
    if (!editingConfig) return;

    const error = validateField(field, value);
    if (error) {
      setLocalErrors({ ...localErrors, [field]: error });
    } else {
      const { [field]: removed, ...rest } = localErrors;
      setLocalErrors(rest);
    }

    const newConfig = JSON.parse(JSON.stringify(editingConfig));
    const keys = field.split('.');
    let obj = newConfig;
    for (let i = 0; i < keys.length - 1; i++) {
      obj = obj[keys[i]];
    }
    obj[keys[keys.length - 1]] = value;

    setEditingConfig(newConfig);
  };

  const handleSave = async () => {
    if (!editingConfig || Object.keys(localErrors).length > 0) return;

    try {
      await updateConfig(currentLayer, editingConfig);
      setHasChanges(false);
    } catch (error) {
      console.error('保存配置失败:', error);
    }
  };

  const handleReset = async () => {
    try {
      await resetConfig(currentLayer);
      setShowResetConfirm(false);
      setLocalErrors({});
      setHasChanges(false);
    } catch (error) {
      console.error('重置配置失败:', error);
    }
  };

  const handleCancel = () => {
    if (config) {
      setEditingConfig(JSON.parse(JSON.stringify(config)));
      setLocalErrors({});
      setHasChanges(false);
    }
  };

  if (loading && !config) {
    return (
      <div className="config-panel loading">
        <div className="spinner" />
        <span>加载配置...</span>
      </div>
    );
  }

  if (!editingConfig) {
    return <div className="config-panel">配置加载失败</div>;
  }

  return (
    <div className="config-panel">
      <div className="config-header">
        <h3>应用配置</h3>
        <div className="layer-tabs">
          <button
            className={currentLayer === 'System' ? 'active' : ''}
            onClick={() => setCurrentLayer('System')}
          >
            系统默认
          </button>
          <button
            className={currentLayer === 'Workspace' ? 'active' : ''}
            onClick={() => setCurrentLayer('Workspace')}
          >
            工作区
          </button>
          <button
            className={currentLayer === 'Project' ? 'active' : ''}
            onClick={() => setCurrentLayer('Project')}
          >
            项目
          </button>
        </div>
      </div>

      {error && (
        <div className="error-message" role="alert">
          {error}
        </div>
      )}

      {currentLayer === 'System' && (
        <div className="info-message">
          系统默认配置为只读，请切换到工作区或项目层进行修改
        </div>
      )}

      <div className="config-sections">
        {/* 日志配置 */}
        <section className="config-section">
          <h4>日志配置</h4>
          <div className="config-fields">
            <div className="field">
              <label>日志级别</label>
              <select
                value={editingConfig.logging.level}
                onChange={(e) => handleFieldChange('logging.level', e.target.value)}
                disabled={currentLayer === 'System'}
              >
                <option value="trace">trace（最详细）</option>
                <option value="debug">debug（调试）</option>
                <option value="info">info（普通）</option>
                <option value="warn">warn（警告）</option>
                <option value="error">error（错误）</option>
              </select>
              {localErrors['logging.level'] && (
                <span className="field-error">{localErrors['logging.level']}</span>
              )}
            </div>

            <div className="field">
              <label>最大文件大小 (MB)</label>
              <input
                type="number"
                min="1"
                max="100"
                value={editingConfig.logging.max_file_size_mb}
                onChange={(e) =>
                  handleFieldChange('logging.max_file_size_mb', parseInt(e.target.value))
                }
                disabled={currentLayer === 'System'}
              />
              {localErrors['logging.max_file_size_mb'] && (
                <span className="field-error">{localErrors['logging.max_file_size_mb']}</span>
              )}
            </div>

            <div className="field">
              <label>最大备份数</label>
              <input
                type="number"
                min="1"
                max="20"
                value={editingConfig.logging.max_backups}
                onChange={(e) =>
                  handleFieldChange('logging.max_backups', parseInt(e.target.value))
                }
                disabled={currentLayer === 'System'}
              />
              {localErrors['logging.max_backups'] && (
                <span className="field-error">{localErrors['logging.max_backups']}</span>
              )}
            </div>
          </div>
        </section>

        {/* 更新配置 */}
        <section className="config-section">
          <h4>更新配置</h4>
          <div className="config-fields">
            <div className="field">
              <label>更新通道</label>
              <select
                value={editingConfig.updates.channel}
                onChange={(e) => handleFieldChange('updates.channel', e.target.value)}
                disabled={currentLayer === 'System'}
              >
                <option value="stable">stable（稳定版）</option>
                <option value="beta">beta（测试版）</option>
                <option value="dev">dev（开发版）</option>
              </select>
              {localErrors['updates.channel'] && (
                <span className="field-error">{localErrors['updates.channel']}</span>
              )}
            </div>

            <div className="field checkbox-field">
              <label>
                <input
                  type="checkbox"
                  checked={editingConfig.updates.auto_check}
                  onChange={(e) => handleFieldChange('updates.auto_check', e.target.checked)}
                  disabled={currentLayer === 'System'}
                />
                自动检查更新
              </label>
            </div>
          </div>
        </section>

        {/* Engine 配置 */}
        <section className="config-section">
          <h4>Engine 配置</h4>
          <div className="config-fields">
            <div className="field">
              <label>启动超时 (秒)</label>
              <input
                type="number"
                min="5"
                max="300"
                value={editingConfig.engine.startup_timeout_secs}
                onChange={(e) =>
                  handleFieldChange('engine.startup_timeout_secs', parseInt(e.target.value))
                }
                disabled={currentLayer === 'System'}
              />
              {localErrors['engine.startup_timeout_secs'] && (
                <span className="field-error">{localErrors['engine.startup_timeout_secs']}</span>
              )}
            </div>
          </div>
        </section>

        {/* 网络配置 */}
        <section className="config-section">
          <h4>网络配置</h4>
          <div className="config-fields">
            <div className="field">
              <label>连接超时 (秒)</label>
              <input
                type="number"
                min="1"
                max="60"
                value={editingConfig.network.connect_timeout_secs}
                onChange={(e) =>
                  handleFieldChange('network.connect_timeout_secs', parseInt(e.target.value))
                }
                disabled={currentLayer === 'System'}
              />
              {localErrors['network.connect_timeout_secs'] && (
                <span className="field-error">{localErrors['network.connect_timeout_secs']}</span>
              )}
            </div>

            <div className="field">
              <label>请求超时 (秒)</label>
              <input
                type="number"
                min="5"
                max="300"
                value={editingConfig.network.request_timeout_secs}
                onChange={(e) =>
                  handleFieldChange('network.request_timeout_secs', parseInt(e.target.value))
                }
                disabled={currentLayer === 'System'}
              />
              {localErrors['network.request_timeout_secs'] && (
                <span className="field-error">{localErrors['network.request_timeout_secs']}</span>
              )}
            </div>
          </div>
        </section>
      </div>

      {/* 操作按钮 */}
      {currentLayer !== 'System' && (
        <div className="config-actions">
          <button
            onClick={() => setShowResetConfirm(true)}
            disabled={loading}
            className="btn-reset"
          >
            重置到默认值
          </button>
          <div className="right-actions">
            {hasChanges && (
              <button onClick={handleCancel} disabled={loading}>
                取消
              </button>
            )}
            <button
              onClick={handleSave}
              disabled={loading || !hasChanges || Object.keys(localErrors).length > 0}
              className="btn-primary"
            >
              保存
            </button>
          </div>
        </div>
      )}

      {/* 重置确认对话框 */}
      {showResetConfirm && (
        <div className="dialog-overlay" onClick={() => setShowResetConfirm(false)}>
          <div className="dialog" onClick={(e) => e.stopPropagation()}>
            <h4>确认重置</h4>
            <p>确定要将 {currentLayer === 'Workspace' ? '工作区' : '项目'} 配置重置到默认值吗？</p>
            <p className="warning">此操作将清除所有自定义配置，不可撤销。</p>
            <div className="dialog-actions">
              <button onClick={() => setShowResetConfirm(false)}>取消</button>
              <button onClick={handleReset} disabled={loading} className="btn-danger">
                重置
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
