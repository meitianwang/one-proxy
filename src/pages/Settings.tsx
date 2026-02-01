import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppConfig {
  host: string;
  port: number;
  debug: boolean;
  "auth-dir": string;
  "api-keys": string[];
  "proxy-url": string;
  "request-retry": number;
  routing: {
    strategy: string;
  };
}

export function Settings() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [newApiKey, setNewApiKey] = useState("");

  useEffect(() => {
    fetchConfig();
  }, []);

  async function fetchConfig() {
    try {
      setLoading(true);
      const result = await invoke<AppConfig>("get_config");
      setConfig(result);
    } catch (error) {
      console.error("Failed to fetch config:", error);
    } finally {
      setLoading(false);
    }
  }

  async function handleSave() {
    if (!config) return;

    try {
      setSaving(true);
      await invoke("save_config", { config });
      alert("配置已保存");
    } catch (error) {
      console.error("Failed to save config:", error);
      alert(`保存失败: ${error}`);
    } finally {
      setSaving(false);
    }
  }

  function handleAddApiKey() {
    if (!config || !newApiKey.trim()) return;

    setConfig({
      ...config,
      "api-keys": [...config["api-keys"], newApiKey.trim()],
    });
    setNewApiKey("");
  }

  function handleRemoveApiKey(index: number) {
    if (!config) return;

    setConfig({
      ...config,
      "api-keys": config["api-keys"].filter((_, i) => i !== index),
    });
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <p className="text-gray-500 dark:text-gray-400">加载中...</p>
      </div>
    );
  }

  if (!config) {
    return (
      <div className="flex items-center justify-center h-64">
        <p className="text-red-500">加载配置失败</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-bold text-gray-800 dark:text-white">设置</h2>
        <button
          onClick={handleSave}
          disabled={saving}
          className="px-4 py-2 bg-blue-500 hover:bg-blue-600 disabled:bg-blue-300 text-white rounded-lg font-medium transition-colors"
        >
          {saving ? "保存中..." : "保存配置"}
        </button>
      </div>

      {/* Server Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          服务器设置
        </h3>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              监听地址
            </label>
            <input
              type="text"
              value={config.host}
              onChange={(e) => setConfig({ ...config, host: e.target.value })}
              placeholder="0.0.0.0"
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              端口
            </label>
            <input
              type="number"
              value={config.port}
              onChange={(e) =>
                setConfig({ ...config, port: parseInt(e.target.value) || 8417 })
              }
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            />
          </div>
        </div>
      </div>

      {/* API Keys */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          API Keys
        </h3>
        <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
          用于客户端访问代理服务的认证密钥
        </p>

        <div className="flex gap-2 mb-4">
          <input
            type="text"
            value={newApiKey}
            onChange={(e) => setNewApiKey(e.target.value)}
            placeholder="输入新的 API Key"
            className="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
          />
          <button
            onClick={handleAddApiKey}
            className="px-4 py-2 bg-green-500 hover:bg-green-600 text-white rounded-lg font-medium transition-colors"
          >
            添加
          </button>
        </div>

        {config["api-keys"].length === 0 ? (
          <p className="text-gray-500 dark:text-gray-400 text-sm">
            暂无 API Key，任何请求都将被允许
          </p>
        ) : (
          <div className="space-y-2">
            {config["api-keys"].map((key, index) => (
              <div
                key={index}
                className="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-700 rounded-lg"
              >
                <code className="text-sm text-gray-800 dark:text-gray-200">
                  {key.slice(0, 8)}...{key.slice(-4)}
                </code>
                <button
                  onClick={() => handleRemoveApiKey(index)}
                  className="text-red-500 hover:text-red-600 text-sm"
                >
                  删除
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Advanced Settings */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          高级设置
        </h3>
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              代理 URL
            </label>
            <input
              type="text"
              value={config["proxy-url"]}
              onChange={(e) =>
                setConfig({ ...config, "proxy-url": e.target.value })
              }
              placeholder="socks5://127.0.0.1:1080"
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              请求重试次数
            </label>
            <input
              type="number"
              value={config["request-retry"]}
              onChange={(e) =>
                setConfig({
                  ...config,
                  "request-retry": parseInt(e.target.value) || 3,
                })
              }
              min={0}
              max={10}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              路由策略
            </label>
            <select
              value={config.routing.strategy}
              onChange={(e) =>
                setConfig({
                  ...config,
                  routing: { ...config.routing, strategy: e.target.value },
                })
              }
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            >
              <option value="round-robin">轮询 (Round Robin)</option>
              <option value="fill-first">填充优先 (Fill First)</option>
            </select>
          </div>

          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="debug"
              checked={config.debug}
              onChange={(e) => setConfig({ ...config, debug: e.target.checked })}
              className="w-4 h-4"
            />
            <label
              htmlFor="debug"
              className="text-sm text-gray-700 dark:text-gray-300"
            >
              启用调试模式
            </label>
          </div>
        </div>
      </div>
    </div>
  );
}
