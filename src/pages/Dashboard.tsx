import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ServerStatus } from "../App";

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

interface DashboardProps {
  serverStatus: ServerStatus;
  onStatusChange: () => void;
}

export function Dashboard({ serverStatus, onStatusChange }: DashboardProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);

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

  async function saveConfig(newConfig: AppConfig) {
    try {
      await invoke("save_config", { config: newConfig });
      setConfig(newConfig);
    } catch (error) {
      console.error("Failed to save config:", error);
      alert(`保存失败: ${error}`);
    }
  }

  async function handleStartServer() {
    try {
      await invoke("start_server");
      onStatusChange();
    } catch (error) {
      console.error("Failed to start server:", error);
      alert(`启动服务器失败: ${error}`);
    }
  }

  async function handleStopServer() {
    try {
      await invoke("stop_server");
      onStatusChange();
    } catch (error) {
      console.error("Failed to stop server:", error);
      alert(`停止服务器失败: ${error}`);
    }
  }

  async function handleGenerateApiKey() {
    if (!config) return;
    const newKey = "sk-" + Array.from(crypto.getRandomValues(new Uint8Array(24)))
      .map(b => b.toString(16).padStart(2, "0"))
      .join("");
    const newConfig = { ...config, "api-keys": [newKey] };
    await saveConfig(newConfig);
  }

  async function handleCopyApiKey() {
    if (!config || config["api-keys"].length === 0) return;
    await navigator.clipboard.writeText(config["api-keys"][0]);
  }

  function isLanAccess() {
    return config?.host === "0.0.0.0";
  }

  async function toggleLanAccess() {
    if (!config) return;
    const newHost = isLanAccess() ? "127.0.0.1" : "0.0.0.0";
    await saveConfig({ ...config, host: newHost });
  }

  async function handlePortChange(port: number) {
    if (!config) return;
    await saveConfig({ ...config, port });
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <p className="text-gray-500 dark:text-gray-400">加载中...</p>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-gray-800 dark:text-white">仪表盘</h2>

      {/* Service Configuration Card */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <div className="flex items-center gap-3">
            <svg className="w-6 h-6 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
            <span className="text-lg font-semibold text-gray-800 dark:text-white">服务配置</span>
            <span className="flex items-center gap-2 text-sm text-gray-500 dark:text-gray-400">
              <span className={`w-2 h-2 rounded-full ${serverStatus.running ? "bg-green-500" : "bg-gray-400"}`} />
              {serverStatus.running ? "服务运行中" : "服务已停止"}
            </span>
          </div>
          <button
            onClick={serverStatus.running ? handleStopServer : handleStartServer}
            className={`px-4 py-2 rounded-lg font-medium flex items-center gap-2 transition-colors ${
              serverStatus.running
                ? "bg-red-500 hover:bg-red-600 text-white"
                : "bg-blue-500 hover:bg-blue-600 text-white"
            }`}
          >
            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5.636 18.364a9 9 0 010-12.728m12.728 0a9 9 0 010 12.728m-9.9-2.829a5 5 0 010-7.07m7.072 0a5 5 0 010 7.07M13 12a1 1 0 11-2 0 1 1 0 012 0z" />
            </svg>
            {serverStatus.running ? "停止服务" : "启动服务"}
          </button>
        </div>

        {/* Settings Grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {/* Port */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              监听端口
            </label>
            <input
              type="number"
              value={config?.port ?? 8417}
              onChange={(e) => handlePortChange(parseInt(e.target.value) || 8417)}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
            />
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              默认 8417，修改端口需重启服务
            </p>
          </div>

          {/* LAN Access */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
              允许局域网访问
            </label>
            <div className="flex items-center justify-between p-3 border border-gray-300 dark:border-gray-600 rounded-lg">
              <div className="flex items-center gap-2">
                <span className="text-sm text-gray-600 dark:text-gray-300">
                  {isLanAccess() ? "监听 0.0.0.0，局域网可访问" : "仅监听 127.0.0.1，仅本机可访问"}
                </span>
              </div>
              <button
                onClick={toggleLanAccess}
                className={`relative w-12 h-6 rounded-full transition-colors ${
                  isLanAccess() ? "bg-blue-500" : "bg-gray-300 dark:bg-gray-600"
                }`}
              >
                <span
                  className={`absolute top-1 w-4 h-4 bg-white rounded-full transition-transform ${
                    isLanAccess() ? "left-7" : "left-1"
                  }`}
                />
              </button>
            </div>
          </div>
        </div>

        {/* API Key Section */}
        <div className="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
          <div className="flex items-center justify-between mb-2">
            <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
              API 密钥
            </label>
            <span className="text-xs text-gray-500 dark:text-gray-400">
              {config?.["api-keys"]?.length ? "已启用" : "未设置（开放访问）"}
            </span>
          </div>
          <div className="flex gap-2">
            <input
              type="text"
              value={config?.["api-keys"]?.[0] ?? ""}
              readOnly
              placeholder="未设置 API 密钥"
              className="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-50 dark:bg-gray-700 text-gray-800 dark:text-white font-mono text-sm"
            />
            <button
              onClick={handleGenerateApiKey}
              className="p-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700"
              title="生成新密钥"
            >
              <svg className="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </button>
            <button
              onClick={handleCopyApiKey}
              className="p-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700"
              title="复制"
            >
              <svg className="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
              </svg>
            </button>
          </div>
          <p className="mt-1 text-xs text-orange-500">
            注意：请妥善保管您的 API 密钥，不要泄露给他人。
          </p>
        </div>
      </div>

      {/* API Endpoints */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
          <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400">
            OpenAI 兼容
          </h4>
          <p className="mt-2 text-sm text-gray-600 dark:text-gray-300">
            /v1/chat/completions
          </p>
          <p className="text-sm text-gray-600 dark:text-gray-300">/v1/models</p>
        </div>

        <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
          <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400">
            Claude 兼容
          </h4>
          <p className="mt-2 text-sm text-gray-600 dark:text-gray-300">
            /v1/messages
          </p>
        </div>
      </div>
    </div>
  );
}
