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
  const [selectedProtocol, setSelectedProtocol] = useState<"openai" | "anthropic" | "gemini">("openai");
  const [selectedModel, setSelectedModel] = useState("gemini-2.5-flash");
  const [models, setModels] = useState<{ id: string; name: string; desc: string }[]>([]);
  const [copied, setCopied] = useState(false);

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

  async function fetchModels() {
    try {
      const url = `http://127.0.0.1:${config?.port ?? 8417}/v1/models`;
      const headers: Record<string, string> = { "Content-Type": "application/json" };
      if (config?.["api-keys"]?.[0]) {
        headers["Authorization"] = `Bearer ${config["api-keys"][0]}`;
      }
      const response = await fetch(url, { headers });
      if (response.ok) {
        const data = await response.json();
        if (data.data && Array.isArray(data.data)) {
          const modelList = data.data.map((m: { id: string; owned_by?: string }) => ({
            id: m.id,
            name: m.id,
            desc: m.owned_by || "",
          }));
          setModels(modelList);
          if (modelList.length > 0 && !modelList.find((m: { id: string }) => m.id === selectedModel)) {
            setSelectedModel(modelList[0].id);
          }
        }
      }
    } catch (error) {
      console.error("Failed to fetch models:", error);
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
      const result = await invoke<ClaudeCodeConfig | null>("get_claude_code_config");
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
              {serverStatus.running ? `服务运行中 :${serverStatus.port}` : "服务已停止"}
            </span>
          </div>
          <button
            onClick={serverStatus.running ? handleStopServer : handleStartServer}
            className={`px-4 py-2 rounded-lg font-medium flex items-center gap-2 transition-colors ${serverStatus.running
              ? "bg-red-500 hover:bg-red-600 text-white"
              : "bg-gray-800 hover:bg-gray-900 dark:bg-gray-700 dark:hover:bg-gray-600 text-white"
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
                className={`relative w-12 h-6 rounded-full transition-colors ${isLanAccess() ? "bg-gray-800 dark:bg-gray-600" : "bg-gray-300 dark:bg-gray-600"
                  }`}
              >
                <span
                  className={`absolute top-1 w-4 h-4 bg-white rounded-full transition-transform ${isLanAccess() ? "left-7" : "left-1"
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

      {/* Multi-Protocol Support */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <div className="flex items-center gap-3 mb-4">
          <div className="w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-700 flex items-center justify-center">
            <svg className="w-5 h-5 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
            </svg>
          </div>
          <div>
            <h3 className="text-lg font-semibold text-gray-800 dark:text-white">多协议支持</h3>
            <p className="text-sm text-gray-500 dark:text-gray-400">支持 OpenAI、Anthropic 和 Gemini 协议</p>
          </div>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          {/* Protocol Selection */}
          <div>
            <div className="grid grid-cols-3 gap-3 mb-4">
              <button
                onClick={() => setSelectedProtocol("openai")}
                className={`p-3 rounded-lg border text-left transition-colors ${selectedProtocol === "openai"
                  ? "border-gray-800 bg-gray-100 dark:bg-gray-700 dark:border-gray-500"
                  : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700"
                  }`}
              >
                <p className={`text-sm font-medium ${selectedProtocol === "openai" ? "text-gray-800 dark:text-white" : "text-gray-800 dark:text-white"}`}>
                  OpenAI 协议
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">/v1/chat/completions</p>
              </button>
              <button
                onClick={() => setSelectedProtocol("anthropic")}
                className={`p-3 rounded-lg border text-left transition-colors ${selectedProtocol === "anthropic"
                  ? "border-gray-800 bg-gray-100 dark:bg-gray-700 dark:border-gray-500"
                  : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700"
                  }`}
              >
                <p className={`text-sm font-medium ${selectedProtocol === "anthropic" ? "text-gray-800 dark:text-white" : "text-gray-800 dark:text-white"}`}>
                  Anthropic 协议
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">/v1/messages</p>
              </button>
              <button
                onClick={() => setSelectedProtocol("gemini")}
                className={`p-3 rounded-lg border text-left transition-colors ${selectedProtocol === "gemini"
                  ? "border-gray-800 bg-gray-100 dark:bg-gray-700 dark:border-gray-500"
                  : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700"
                  }`}
              >
                <p className={`text-sm font-medium ${selectedProtocol === "gemini" ? "text-gray-800 dark:text-white" : "text-gray-800 dark:text-white"}`}>
                  Gemini 协议
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">/gemini/v1beta</p>
              </button>
            </div>

            {/* Model Selection */}
            <div className="mt-4 pt-4 border-t border-gray-200 dark:border-gray-700">
              <p className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">选择模型</p>
              <div className="space-y-2 max-h-40 overflow-y-auto">
                {models.length === 0 ? (
                  <p className="text-sm text-gray-500 dark:text-gray-400 py-2">
                    {serverStatus.running ? "暂无可用模型，请先添加账号" : "请先启动服务"}
                  </p>
                ) : (
                  models.map((model) => (
                    <button
                      key={model.id}
                      onClick={() => setSelectedModel(model.id)}
                      className={`w-full p-2 rounded-lg border text-left text-sm transition-colors ${selectedModel === model.id
                        ? "border-gray-800 bg-gray-100 dark:bg-gray-700 dark:border-gray-500"
                        : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700"
                        }`}
                    >
                      <span className={`font-medium ${selectedModel === model.id ? "text-gray-800 dark:text-white" : "text-gray-800 dark:text-white"}`}>
                        {model.name}
                      </span>
                      {model.desc && <span className="text-xs text-gray-500 dark:text-gray-400 ml-2">{model.desc}</span>}
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>

          {/* Curl Command */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">测试命令 (curl)</span>
              <button
                onClick={() => {
                  navigator.clipboard.writeText(curlCommands[selectedProtocol]);
                  setCopied(true);
                  setTimeout(() => setCopied(false), 2000);
                }}
                className="text-xs text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 flex items-center gap-1"
              >
                {copied ? (
                  <>
                    <svg className="w-4 h-4 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                    </svg>
                    <span className="text-green-500">已复制</span>
                  </>
                ) : (
                  <>
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                    </svg>
                    复制
                  </>
                )}
              </button>
            </div>
            <pre className="bg-gray-900 text-gray-100 p-4 rounded-lg text-xs overflow-x-auto whitespace-pre-wrap font-mono">
              {curlCommands[selectedProtocol]}
            </pre>
          </div>
        </div>
      </div>

      {/* Claude Code Config */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-700 flex items-center justify-center">
              <svg className="w-5 h-5 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z" />
              </svg>
            </div>
            <div>
              <h3 className="text-lg font-semibold text-gray-800 dark:text-white">Claude Code 配置</h3>
              <p className="text-sm text-gray-500 dark:text-gray-400">配置 ~/.claude/settings.json 模型映射</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            {claudeConfigSaved && (
              <span className="text-sm text-green-600 dark:text-green-400 flex items-center gap-1">
                <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                </svg>
                已保存
              </span>
            )}
            <button
              onClick={saveClaudeCodeConfig}
              disabled={claudeConfigSaving}
              className="px-4 py-2 bg-gray-800 hover:bg-gray-900 dark:bg-gray-700 dark:hover:bg-gray-600 text-white rounded-lg font-medium disabled:opacity-50 flex items-center gap-2"
            >
              {claudeConfigSaving ? (
                <>
                  <svg className="w-4 h-4 animate-spin" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                  </svg>
                  保存中...
                </>
              ) : (
                "写入配置"
              )}
            </button>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {/* Opus Model */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Opus 模型
            </label>
            <select
              value={claudeConfig.opus_model}
              onChange={(e) => setClaudeConfig({ ...claudeConfig, opus_model: e.target.value })}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white text-sm"
            >
              <option value="">选择模型...</option>
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.id}
                </option>
              ))}
            </select>
          </div>

          {/* Sonnet Model */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Sonnet 模型
            </label>
            <select
              value={claudeConfig.sonnet_model}
              onChange={(e) => setClaudeConfig({ ...claudeConfig, sonnet_model: e.target.value })}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white text-sm"
            >
              <option value="">选择模型...</option>
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.id}
                </option>
              ))}
            </select>
          </div>

          {/* Haiku Model */}
          <div>
            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              Haiku 模型
            </label>
            <select
              value={claudeConfig.haiku_model}
              onChange={(e) => setClaudeConfig({ ...claudeConfig, haiku_model: e.target.value })}
              className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white text-sm"
            >
              <option value="">选择模型...</option>
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.id}
                </option>
              ))}
            </select>
          </div>
        </div>

        <p className="mt-4 text-xs text-gray-500 dark:text-gray-400">
          配置将写入 ~/.claude/settings.json，用于 Claude Code CLI 的模型映射。保存后需要重启 Claude Code 生效。
        </p>
      </div>
    </div>
  );
}
