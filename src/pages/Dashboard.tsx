import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ServerStatus } from "../App";

interface AuthSummary {
  total: number;
  enabled: number;
  by_provider: Record<string, number>;
}

interface DashboardProps {
  serverStatus: ServerStatus;
  onStatusChange: () => void;
}

export function Dashboard({ serverStatus, onStatusChange }: DashboardProps) {
  const [authSummary, setAuthSummary] = useState<AuthSummary | null>(null);

  useEffect(() => {
    fetchAuthSummary();
  }, []);

  async function fetchAuthSummary() {
    try {
      const summary = await invoke<AuthSummary>("get_auth_summary");
      setAuthSummary(summary);
    } catch (error) {
      console.error("Failed to fetch auth summary:", error);
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

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-gray-800 dark:text-white">仪表盘</h2>

      {/* Server Status Card */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          服务器状态
        </h3>
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div
              className={`w-4 h-4 rounded-full ${
                serverStatus.running ? "bg-green-500" : "bg-red-500"
              }`}
            />
            <div>
              <p className="text-gray-800 dark:text-white font-medium">
                {serverStatus.running ? "运行中" : "已停止"}
              </p>
              {serverStatus.running && (
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  监听地址: {serverStatus.host}:{serverStatus.port}
                </p>
              )}
            </div>
          </div>
          <button
            onClick={serverStatus.running ? handleStopServer : handleStartServer}
            className={`px-4 py-2 rounded-lg font-medium transition-colors ${
              serverStatus.running
                ? "bg-red-500 hover:bg-red-600 text-white"
                : "bg-green-500 hover:bg-green-600 text-white"
            }`}
          >
            {serverStatus.running ? "停止服务" : "启动服务"}
          </button>
        </div>
      </div>

      {/* Quick Info Cards */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
          <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400">
            已登录账户
          </h4>
          <p className="mt-2 text-3xl font-bold text-gray-800 dark:text-white">
            {authSummary?.total ?? 0}
          </p>
          <p className="text-sm text-green-600 dark:text-green-400">
            {authSummary?.enabled ?? 0} 个已启用
          </p>
        </div>

        <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
          <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400">
            API 端点
          </h4>
          <p className="mt-2 text-xl font-semibold text-gray-800 dark:text-white">
            http://localhost:{serverStatus.port}
          </p>
        </div>

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
