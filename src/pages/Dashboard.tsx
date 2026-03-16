import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ServerStatus } from "../App";
import {
  Server,
  Settings2,
  Key,
  Globe,
  Box,
  Copy,
  Check,
  Terminal,
  Play,
  Square,
  Loader2,
  Save,
  RefreshCw,
} from "lucide-react";

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

interface ClaudeCodeConfig {
  opus_model: string;
  sonnet_model: string;
  haiku_model: string;
}

interface DashboardProps {
  serverStatus: ServerStatus;
  onStatusChange: () => void;
}

export function Dashboard({ serverStatus, onStatusChange }: DashboardProps) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedProtocol, setSelectedProtocol] = useState<
    "openai" | "anthropic" | "gemini"
  >("openai");
  const [selectedModel, setSelectedModel] = useState("gemini-2.5-flash");
  const [models, setModels] = useState<
    { id: string; name: string; desc: string }[]
  >([]);
  const [copied, setCopied] = useState(false);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [serverLoading, setServerLoading] = useState<
    "starting" | "stopping" | null
  >(null);

  // Claude Code config state
  const [claudeConfig, setClaudeConfig] = useState<ClaudeCodeConfig>({
    opus_model: "",
    sonnet_model: "",
    haiku_model: "",
  });
  const [claudeConfigSaving, setClaudeConfigSaving] = useState(false);
  const [claudeConfigSaved, setClaudeConfigSaved] = useState(false);

  const baseUrl = `http://127.0.0.1:${config?.port ?? 8417}`;
  const apiKey = config?.["api-keys"]?.[0] ?? "your-api-key";

  const curlCommands = {
    openai: `curl -X POST ${baseUrl}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${apiKey}" \\
  -d '{
    "model": "${selectedModel}",
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
    anthropic: `curl -X POST ${baseUrl}/v1/messages \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${apiKey}" \\
  -H "anthropic-version: 2023-06-01" \\
  -d '{
    "model": "${selectedModel}",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello"}]
  }'`,
    gemini: `curl -X POST ${baseUrl}/gemini/v1beta/models/${selectedModel}:generateContent \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer ${apiKey}" \\
  -d '{
    "contents": [{"role": "user", "parts": [{"text": "Hello"}]}]
  }'`,
  };

  useEffect(() => {
    fetchConfig();
    fetchClaudeCodeConfig();
  }, []);

  useEffect(() => {
    if (serverStatus.running && config) {
      fetchModels();
    }
  }, [serverStatus.running, config]);

  // Auto-reset loading state when server status changes
  useEffect(() => {
    setServerLoading(null);
  }, [serverStatus.running]);

  async function fetchModels() {
    setModelsLoading(true);
    try {
      const url = `http://127.0.0.1:${config?.port ?? 8417}/v1/models`;
      const headers: Record<string, string> = {
        "Content-Type": "application/json",
      };
      if (config?.["api-keys"]?.[0]) {
        headers["Authorization"] = `Bearer ${config["api-keys"][0]}`;
      }
      const response = await fetch(url, { headers });
      if (response.ok) {
        const data = await response.json();
        if (data.data && Array.isArray(data.data)) {
          const modelList = data.data.map(
            (m: { id: string; owned_by?: string }) => ({
              id: m.id,
              name: m.id,
              desc: m.owned_by || "",
            }),
          );
          setModels(modelList);
          if (
            modelList.length > 0 &&
            !modelList.find((m: { id: string }) => m.id === selectedModel)
          ) {
            setSelectedModel(modelList[0].id);
          }
        }
      }
    } catch (error) {
      console.error("Failed to fetch models:", error);
    } finally {
      setModelsLoading(false);
    }
  }

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

  async function fetchClaudeCodeConfig() {
    try {
      const result = await invoke<ClaudeCodeConfig | null>(
        "get_claude_code_config",
      );
      if (result) {
        setClaudeConfig(result);
      }
    } catch (error) {
      console.error("Failed to fetch Claude Code config:", error);
    }
  }

  async function saveClaudeCodeConfig() {
    setClaudeConfigSaving(true);
    setClaudeConfigSaved(false);
    try {
      await invoke("save_claude_code_config", { claudeConfig });
      setClaudeConfigSaved(true);
      setTimeout(() => setClaudeConfigSaved(false), 2000);
    } catch (error) {
      console.error("Failed to save Claude Code config:", error);
      alert(`保存失败: ${error}`);
    } finally {
      setClaudeConfigSaving(false);
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
    setServerLoading("starting");
    try {
      await invoke("start_server");
      onStatusChange();
    } catch (error) {
      console.error("Failed to start server:", error);
      alert(`启动服务器失败: ${error}`);
      setServerLoading(null);
    }
  }

  async function handleStopServer() {
    setServerLoading("stopping");
    try {
      await invoke("stop_server");
      onStatusChange();
    } catch (error) {
      console.error("Failed to stop server:", error);
      alert(`停止服务器失败: ${error}`);
      setServerLoading(null);
    }
  }

  async function handleGenerateApiKey() {
    if (!config) return;
    const newKey =
      "sk-" +
      Array.from(crypto.getRandomValues(new Uint8Array(24)))
        .map((b) => b.toString(16).padStart(2, "0"))
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
      <div className="flex items-center justify-center h-[calc(100vh-8rem)]">
        <div className="flex flex-col items-center gap-4 animate-pulse">
          <Loader2 className="w-10 h-10 text-blue-500 animate-spin" />
          <p className="text-gray-500 dark:text-gray-400 font-medium">
            配置加载中...
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-6xl mx-auto space-y-8 animate-in mt-4">
      {/* Page Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-3xl font-black tracking-tight text-gray-900 dark:text-white">
            仪表盘
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            管理您的核心服务状态与多重协议配置
          </p>
        </div>
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-8">
        {/* Service Configuration Options */}
        <div className="space-y-8 flex flex-col">
          {/* Card 1: Server Status */}
          <div className="bg-white/70 dark:bg-gray-900/60 backdrop-blur-xl rounded-3xl p-6 md:p-8 shadow-sm border border-gray-200/50 dark:border-gray-800/50 relative overflow-hidden group">
            <div className="absolute top-0 right-0 w-64 h-64 bg-blue-100/30 dark:bg-blue-900/10 rounded-full blur-3xl -mr-20 -mt-20 pointer-events-none transition-transform group-hover:scale-110 duration-700 ease-out" />

            <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between mb-8 relative z-10 gap-4">
              <div className="flex items-center gap-4">
                <div
                  className={`w-14 h-14 rounded-2xl flex items-center justify-center shadow-md transition-all duration-500 ${serverStatus.running ? "bg-gradient-to-br from-emerald-400 to-green-600 shadow-green-500/20" : "bg-gradient-to-br from-gray-400 to-gray-600 shadow-gray-500/20"}`}
                >
                  <Server className="w-7 h-7 text-white" />
                </div>
                <div>
                  <h3 className="text-xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
                    服务配置
                  </h3>
                  <div className="flex items-center gap-2 mt-1">
                    <span className="relative flex h-2.5 w-2.5">
                      {serverStatus.running && (
                        <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                      )}
                      <span
                        className={`relative inline-flex rounded-full h-2.5 w-2.5 ${serverStatus.running ? "bg-green-500" : "bg-gray-400"}`}
                      ></span>
                    </span>
                    <span className="text-sm font-medium text-gray-600 dark:text-gray-400">
                      {serverStatus.running
                        ? `运行中 :${serverStatus.port}`
                        : "已停止"}
                    </span>
                  </div>
                </div>
              </div>

              <button
                onClick={
                  serverStatus.running ? handleStopServer : handleStartServer
                }
                disabled={serverLoading !== null}
                className={`relative overflow-hidden px-6 py-2.5 rounded-xl font-bold flex items-center gap-2 transition-all duration-300 transform active:scale-95 disabled:scale-100 shadow-lg ${
                  serverStatus.running
                    ? "bg-rose-500 hover:bg-rose-600 text-white shadow-rose-500/25"
                    : "bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100"
                } ${serverLoading ? "opacity-70 cursor-not-allowed" : "hover:-translate-y-0.5"}`}
              >
                {serverLoading ? (
                  <Loader2 className="w-5 h-5 animate-spin" />
                ) : serverStatus.running ? (
                  <Square className="w-4 h-4 fill-current" />
                ) : (
                  <Play className="w-4 h-4 fill-current" />
                )}
                {serverLoading === "starting"
                  ? "启动中"
                  : serverLoading === "stopping"
                    ? "停止中"
                    : serverStatus.running
                      ? "停止"
                      : "启动"}
              </button>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-5 relative z-10">
              <div className="p-4 rounded-2xl bg-gray-50/50 dark:bg-gray-800/40 border border-gray-200/50 dark:border-gray-700/50">
                <label className="flex items-center gap-2 text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">
                  <Settings2 className="w-4 h-4" />
                  监听端口
                </label>
                <input
                  type="number"
                  value={config?.port ?? 8417}
                  onChange={(e) =>
                    handlePortChange(parseInt(e.target.value) || 8417)
                  }
                  className="w-full px-4 py-2.5 border border-gray-300/60 dark:border-gray-600/60 rounded-xl bg-white dark:bg-gray-900 text-gray-900 dark:text-white font-mono font-medium focus:ring-2 focus:ring-blue-500/40 focus:border-blue-500 transition-all shadow-sm outline-none"
                />
                <p className="mt-2 text-[11px] text-gray-500 dark:text-gray-400 font-medium">
                  重启生效
                </p>
              </div>

              <div className="p-4 rounded-2xl bg-gray-50/50 dark:bg-gray-800/40 border border-gray-200/50 dark:border-gray-700/50">
                <label className="flex items-center justify-between text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">
                  <span className="flex items-center gap-2">
                    <Globe className="w-4 h-4" /> 局域网访问
                  </span>
                </label>
                <div className="flex items-center justify-between px-3 py-2.5 bg-white dark:bg-gray-900 border border-gray-300/60 dark:border-gray-600/60 rounded-xl shadow-sm">
                  <span className="text-xs font-medium text-gray-600 dark:text-gray-300 truncate mr-2">
                    {isLanAccess() ? "0.0.0.0 (对外)" : "127.0.0.1 (本地)"}
                  </span>
                  <button
                    onClick={toggleLanAccess}
                    className={`relative w-11 h-6 rounded-full transition-colors flex-shrink-0 duration-300 ${isLanAccess() ? "bg-blue-500 shadow-inner" : "bg-gray-300 dark:bg-gray-600"}`}
                  >
                    <span
                      className={`absolute top-1 bg-white w-4 h-4 rounded-full transition-transform duration-300 shadow-sm ${isLanAccess() ? "left-6" : "left-1"}`}
                    />
                  </button>
                </div>
              </div>
            </div>

            {/* API Key Section */}
            <div className="mt-6 pt-6 border-t border-gray-200/50 dark:border-gray-700/50 relative z-10">
              <div className="flex items-center justify-between mb-3">
                <label className="flex items-center gap-2 text-sm font-semibold text-gray-800 dark:text-gray-200">
                  <Key className="w-4 h-4" /> API 密钥
                </label>
                <span
                  className={`text-[11px] px-2 py-0.5 rounded-full font-bold ${config?.["api-keys"]?.length ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400 border border-emerald-200 dark:border-emerald-800/50" : "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400 border border-orange-200 dark:border-orange-800/50"}`}
                >
                  {config?.["api-keys"]?.length ? "已设置" : "未设置（不安全）"}
                </span>
              </div>

              <div className="flex h-11">
                <input
                  type="text"
                  value={config?.["api-keys"]?.[0] ?? ""}
                  readOnly
                  placeholder="未设置 API 密钥 - 任何人均可访问"
                  className="w-full px-4 rounded-l-xl border-y border-l border-gray-300/80 dark:border-gray-600/80 bg-gray-50/80 dark:bg-gray-800/80 text-gray-900 dark:text-gray-100 font-mono text-sm focus:outline-none placeholder-gray-400 dark:placeholder-gray-500"
                />
                <button
                  onClick={handleGenerateApiKey}
                  className="px-4 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 border-y border-l border-gray-300/80 dark:border-gray-600/80 transition-colors text-gray-600 dark:text-gray-300 outline-none"
                  title="生成新密钥"
                >
                  <RefreshCw className="w-4 h-4" />
                </button>
                <button
                  onClick={handleCopyApiKey}
                  className="px-4 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 border border-gray-300/80 dark:border-gray-600/80 rounded-r-xl transition-colors text-gray-600 dark:text-gray-300 outline-none"
                  title="复制密钥"
                >
                  <Copy className="w-4 h-4" />
                </button>
              </div>
            </div>
          </div>

          {/* Card 3: Claude Code Config */}
          <div className="bg-white/70 dark:bg-gray-900/60 backdrop-blur-xl rounded-3xl p-6 md:p-8 shadow-sm border border-gray-200/50 dark:border-gray-800/50 flex-1">
            <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between mb-8 gap-4">
              <div className="flex flex-col">
                <h3 className="text-xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
                  Claude 客户端配置
                </h3>
                <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mt-1">
                  自动配置 ~/.claude/settings.json
                </p>
              </div>
              <button
                onClick={saveClaudeCodeConfig}
                disabled={claudeConfigSaving}
                className="px-5 py-2.5 bg-gray-900 hover:bg-gray-800 dark:bg-white dark:hover:bg-gray-100 text-white dark:text-gray-900 rounded-xl font-bold disabled:opacity-50 flex items-center gap-2 transition-all shadow-sm active:scale-95 duration-200 cursor-pointer w-full sm:w-auto justify-center"
              >
                {claudeConfigSaving ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : claudeConfigSaved ? (
                  <Check className="w-4 h-4" />
                ) : (
                  <Save className="w-4 h-4" />
                )}
                {claudeConfigSaving
                  ? "保存中"
                  : claudeConfigSaved
                    ? "已保存"
                    : "写入配置"}
              </button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-5">
              {[
                { label: "Opus 模型", prop: "opus_model" },
                { label: "Sonnet 模型", prop: "sonnet_model" },
                { label: "Haiku 模型", prop: "haiku_model" },
              ].map((item) => (
                <div key={item.prop} className="space-y-2">
                  <label className="block text-sm font-bold text-gray-700 dark:text-gray-300">
                    {item.label}
                  </label>
                  <select
                    value={claudeConfig[item.prop as keyof ClaudeCodeConfig]}
                    onChange={(e) =>
                      setClaudeConfig({
                        ...claudeConfig,
                        [item.prop]: e.target.value,
                      })
                    }
                    className="w-full px-3 py-2.5 border border-gray-200 dark:border-gray-700/50 rounded-xl bg-gray-50/50 dark:bg-gray-800/50 text-gray-800 dark:text-white text-sm focus:ring-2 focus:ring-blue-500/40 focus:border-blue-500 outline-none transition-shadow shadow-sm font-medium appearance-none"
                    style={{
                      backgroundImage: `url("data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e")`,
                      backgroundPosition: `right 0.5rem center`,
                      backgroundRepeat: `no-repeat`,
                      backgroundSize: `1.5em 1.5em`,
                      paddingRight: `2.5rem`,
                    }}
                  >
                    <option value="">(空)</option>
                    {models.map((model) => (
                      <option key={model.id} value={model.id}>
                        {model.id}
                      </option>
                    ))}
                  </select>
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Card 2: Multi-Protocol and Test */}
        <div className="bg-white/70 dark:bg-gray-900/60 backdrop-blur-xl rounded-3xl p-6 md:p-8 shadow-sm border border-gray-200/50 dark:border-gray-800/50 relative overflow-hidden flex flex-col">
          <div className="absolute top-0 right-0 w-64 h-64 bg-purple-100/30 dark:bg-purple-900/10 rounded-full blur-3xl -mr-20 -mt-20 pointer-events-none" />

          <div className="flex items-center justify-between mb-8 relative z-10">
            <div className="flex items-center gap-4">
              <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center shadow-lg shadow-purple-500/20">
                <Box className="w-7 h-7 text-white" />
              </div>
              <div>
                <h3 className="text-xl font-bold text-gray-900 dark:text-white">
                  API 与协议测试
                </h3>
                <p className="text-sm font-medium text-gray-500 dark:text-gray-400 mt-1">
                  自动转换并代理多重主流协议
                </p>
              </div>
            </div>
            {serverStatus.running && (
              <button
                onClick={fetchModels}
                disabled={modelsLoading}
                title="刷新模型列表"
                className="p-2.5 rounded-xl bg-gray-100/80 hover:bg-gray-200/80 dark:bg-gray-800 dark:hover:bg-gray-700 text-gray-500 dark:text-gray-400 transition-all active:scale-95 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <RefreshCw
                  className={`w-4 h-4 ${modelsLoading ? "animate-spin" : ""}`}
                />
              </button>
            )}
          </div>

          <div className="flex-1 flex flex-col relative z-10">
            {/* Protocol Tabs */}
            <div className="flex gap-2 p-1.5 bg-gray-100/80 dark:bg-gray-800/70 rounded-2xl mb-6 flex-wrap x-overflow-auto shadow-inner">
              {[
                { id: "openai", label: "OpenAI", path: "/v1" },
                { id: "anthropic", label: "Anthropic", path: "/v1" },
                { id: "gemini", label: "Gemini", path: "/v1beta" },
              ].map((proto) => (
                <button
                  key={proto.id}
                  onClick={() => setSelectedProtocol(proto.id as any)}
                  className={`flex-1 min-w-[100px] flex flex-col items-center justify-center py-2.5 px-3 rounded-xl text-sm transition-all duration-300 font-bold ${
                    selectedProtocol === proto.id
                      ? "bg-white dark:bg-gray-700 text-gray-900 dark:text-white shadow-[0_2px_8px_-2px_rgba(0,0,0,0.1)] scale-100"
                      : "text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 hover:bg-white/50 dark:hover:bg-gray-700/50 scale-95"
                  }`}
                >
                  <span>{proto.label}</span>
                  <span
                    className={`text-[10px] font-medium mt-0.5 ${selectedProtocol === proto.id ? "text-gray-500 dark:text-gray-400" : "opacity-0"}`}
                  >
                    {proto.path}
                  </span>
                </button>
              ))}
            </div>

            {/* Model Selection View */}
            <div className="mb-6 flex-1">
              <label className="flex items-center gap-2 text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">
                <Box className="w-4 h-4" /> 选择目标发信模型
              </label>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-3 max-h-56 overflow-y-auto pr-2 custom-scrollbar">
                {models.length === 0 ? (
                  <div className="col-span-1 md:col-span-2 flex flex-col items-center justify-center py-10 px-4 text-center border-2 border-dashed border-gray-200 dark:border-gray-700/50 rounded-2xl bg-gray-50/50 dark:bg-gray-800/30">
                    <Box className="w-8 h-8 text-gray-300 dark:text-gray-600 mb-2" />
                    <p className="text-sm font-medium text-gray-500 dark:text-gray-400">
                      {serverStatus.running
                        ? "暂无可用模型，请前往[账号管理]添加"
                        : "请先启动服务以拉取模型列表"}
                    </p>
                  </div>
                ) : (
                  models.map((model) => (
                    <button
                      key={model.id}
                      onClick={() => setSelectedModel(model.id)}
                      className={`flex flex-col text-left p-3.5 rounded-2xl border transition-all duration-200 ${
                        selectedModel === model.id
                          ? "border-blue-500/50 bg-blue-50/50 dark:bg-blue-900/20 dark:border-blue-500/50 shadow-[0_4px_12px_-4px_rgba(59,130,246,0.15)] ring-1 ring-blue-500/20"
                          : "border-gray-200/60 dark:border-gray-700/50 hover:border-gray-300 dark:hover:border-gray-600 bg-white/50 dark:bg-gray-800/30"
                      }`}
                    >
                      <span
                        className={`text-sm font-bold truncate w-full ${selectedModel === model.id ? "text-blue-700 dark:text-blue-300" : "text-gray-700 dark:text-gray-300"}`}
                      >
                        {model.name}
                      </span>
                      {model.desc && (
                        <span className="text-[11px] font-medium text-gray-500 dark:text-gray-500 truncate w-full mt-1">
                          {model.desc}
                        </span>
                      )}
                    </button>
                  ))
                )}
              </div>
            </div>

            {/* Curl Command Box */}
            <div className="mt-auto">
              <div className="flex items-center justify-between mb-2">
                <label className="flex items-center gap-2 text-sm font-semibold text-gray-700 dark:text-gray-300">
                  <Terminal className="w-4 h-4" /> cURL 命令示例
                </label>
                <button
                  onClick={() => {
                    navigator.clipboard.writeText(
                      curlCommands[selectedProtocol],
                    );
                    setCopied(true);
                    setTimeout(() => setCopied(false), 2000);
                  }}
                  className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] font-bold transition-all ${
                    copied
                      ? "bg-green-100/80 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                      : "bg-gray-100/80 text-gray-600 hover:bg-gray-200/80 dark:bg-gray-800 dark:text-gray-400 dark:hover:bg-gray-700"
                  }`}
                >
                  {copied ? (
                    <Check className="w-3.5 h-3.5" />
                  ) : (
                    <Copy className="w-3.5 h-3.5" />
                  )}
                  {copied ? "已复制" : "复制代码"}
                </button>
              </div>

              <div className="relative group rounded-2xl overflow-hidden shadow-inner border border-gray-200/30 dark:border-gray-800">
                <div className="absolute top-0 left-0 w-full h-8 bg-gradient-to-b from-gray-900/50 to-transparent pointer-events-none z-10" />
                <pre className="relative bg-[#0d1117] text-[#c9d1d9] p-5 pt-4 text-xs overflow-x-auto whitespace-pre-wrap font-mono leading-relaxed custom-scrollbar">
                  <div className="flex gap-2 mb-2">
                    <div className="w-2.5 h-2.5 rounded-full bg-red-500/80" />
                    <div className="w-2.5 h-2.5 rounded-full bg-yellow-500/80" />
                    <div className="w-2.5 h-2.5 rounded-full bg-green-500/80" />
                  </div>
                  <code className="block mt-2">
                    {curlCommands[selectedProtocol]}
                  </code>
                </pre>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
