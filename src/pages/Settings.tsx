import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

interface ProviderPriorityData {
  provider: string;
  priority: number;
  enabled: boolean;
}

interface SettingsData {
  quota_refresh_interval: number;
  model_routing_mode: string;
  provider_priorities: ProviderPriorityData[];
  account_routing_strategy: string;
}

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
  [key: string]: unknown;
}

interface CustomProviderEntry {
  name: string;
  prefix: string | null;
  base_url: string;
  api_keys: string[];
  models: string[];
}

interface CustomProvidersData {
  openai_compatibility: CustomProviderEntry[];
  claude_code_compatibility: CustomProviderEntry[];
}

const QUOTA_INTERVAL_OPTIONS = [
  { value: 1, label: "1 分钟" },
  { value: 5, label: "5 分钟" },
  { value: 10, label: "10 分钟" },
  { value: 15, label: "15 分钟" },
  { value: 30, label: "30 分钟" },
  { value: 60, label: "60 分钟" },
];

const EMPTY_PROVIDER: CustomProviderEntry = {
  name: "",
  prefix: null,
  base_url: "",
  api_keys: [""],
  models: [],
};

export function Settings() {
  const [settings, setSettings] = useState<SettingsData>({
    quota_refresh_interval: 5,
    model_routing_mode: "provider",
    provider_priorities: [],
    account_routing_strategy: "stick-until-exhausted",
  });
  const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
  const [customProviders, setCustomProviders] = useState<CustomProvidersData>({
    openai_compatibility: [],
    claude_code_compatibility: [],
  });
  const [loading, setLoading] = useState(true);
  const [configLoading, setConfigLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [activeTab, setActiveTab] = useState<"general" | "openai" | "claude">("general");

  useEffect(() => {
    loadSettings();
    loadCustomProviders();
    loadConfig();
  }, []);

  async function loadSettings() {
    try {
      const data = await invoke<SettingsData>("get_settings");
      setSettings(data);
    } catch (error) {
      console.error("Failed to load settings:", error);
    } finally {
      setLoading(false);
    }
  }

  async function loadConfig() {
    try {
      const data = await invoke<AppConfig>("get_config");
      setAppConfig(data);
    } catch (error) {
      console.error("Failed to load config:", error);
    } finally {
      setConfigLoading(false);
    }
  }

  async function loadCustomProviders() {
    try {
      const data = await invoke<CustomProvidersData>("get_custom_providers");
      // Ensure api_keys is always an array with at least one empty string for UI
      const normalizeProviders = (providers: CustomProviderEntry[]) =>
        providers.map(p => ({
          ...p,
          api_keys: p.api_keys.length > 0 ? p.api_keys : [""],
        }));
      setCustomProviders({
        openai_compatibility: normalizeProviders(data.openai_compatibility),
        claude_code_compatibility: normalizeProviders(data.claude_code_compatibility),
      });
    } catch (error) {
      console.error("Failed to load custom providers:", error);
    }
  }

  async function handleSave() {
    setSaving(true);
    setSaved(false);
    try {
      // Important: Save appConfig first, then settings
      // This ensures model_routing settings are not overwritten by appConfig
      if (appConfig) {
        await invoke("save_config", { config: appConfig });
      }
      // Filter out empty api_keys before saving
      const cleanProviders = (providers: CustomProviderEntry[]) =>
        providers.map(p => ({
          ...p,
          api_keys: p.api_keys.filter(k => k.trim() !== ""),
        })).filter(p => p.name.trim() !== "" && p.base_url.trim() !== "");
      await invoke("save_custom_providers", {
        data: {
          openai_compatibility: cleanProviders(customProviders.openai_compatibility),
          claude_code_compatibility: cleanProviders(customProviders.claude_code_compatibility),
        },
      });
      // Save settings last to ensure model_routing is preserved
      await invoke("save_settings", { settings });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (error) {
      console.error("Failed to save settings:", error);
      alert(`保存失败: ${error}`);
    } finally {
      setSaving(false);
    }
  }

  function addProvider(type: "openai" | "claude") {
    if (type === "openai") {
      setCustomProviders({
        ...customProviders,
        openai_compatibility: [...customProviders.openai_compatibility, { ...EMPTY_PROVIDER }],
      });
    } else {
      setCustomProviders({
        ...customProviders,
        claude_code_compatibility: [...customProviders.claude_code_compatibility, { ...EMPTY_PROVIDER }],
      });
    }
  }

  function removeProvider(type: "openai" | "claude", index: number) {
    if (type === "openai") {
      setCustomProviders({
        ...customProviders,
        openai_compatibility: customProviders.openai_compatibility.filter((_, i) => i !== index),
      });
    } else {
      setCustomProviders({
        ...customProviders,
        claude_code_compatibility: customProviders.claude_code_compatibility.filter((_, i) => i !== index),
      });
    }
  }

  function updateProvider(type: "openai" | "claude", index: number, field: keyof CustomProviderEntry, value: string | string[] | null) {
    const key = type === "openai" ? "openai_compatibility" : "claude_code_compatibility";
    const providers = [...customProviders[key]];
    providers[index] = { ...providers[index], [field]: value };
    setCustomProviders({ ...customProviders, [key]: providers });
  }

  function addApiKey(type: "openai" | "claude", providerIndex: number) {
    const key = type === "openai" ? "openai_compatibility" : "claude_code_compatibility";
    const providers = [...customProviders[key]];
    providers[providerIndex] = {
      ...providers[providerIndex],
      api_keys: [...providers[providerIndex].api_keys, ""],
    };
    setCustomProviders({ ...customProviders, [key]: providers });
  }

  function updateApiKey(type: "openai" | "claude", providerIndex: number, keyIndex: number, value: string) {
    const key = type === "openai" ? "openai_compatibility" : "claude_code_compatibility";
    const providers = [...customProviders[key]];
    const apiKeys = [...providers[providerIndex].api_keys];
    apiKeys[keyIndex] = value;
    providers[providerIndex] = { ...providers[providerIndex], api_keys: apiKeys };
    setCustomProviders({ ...customProviders, [key]: providers });
  }

  function removeApiKey(type: "openai" | "claude", providerIndex: number, keyIndex: number) {
    const key = type === "openai" ? "openai_compatibility" : "claude_code_compatibility";
    const providers = [...customProviders[key]];
    const apiKeys = providers[providerIndex].api_keys.filter((_, i) => i !== keyIndex);
    providers[providerIndex] = { ...providers[providerIndex], api_keys: apiKeys.length > 0 ? apiKeys : [""] };
    setCustomProviders({ ...customProviders, [key]: providers });
  }

  if (loading || configLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span className="text-gray-500 dark:text-gray-400">加载中...</span>
      </div>
    );
  }

  const renderProviderForm = (type: "openai" | "claude", providers: CustomProviderEntry[]) => (
    <div className="space-y-4">
      {providers.map((provider, index) => (
        <div key={index} className="border border-gray-200 dark:border-gray-700 rounded-lg p-4 space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
              供应商 #{index + 1}
            </span>
            <button
              onClick={() => removeProvider(type, index)}
              className="text-red-500 hover:text-red-700 text-sm"
            >
              删除
            </button>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">名称 *</label>
              <input
                type="text"
                value={provider.name}
                onChange={(e) => updateProvider(type, index, "name", e.target.value)}
                placeholder="例如: DeepSeek"
                className="w-full px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">前缀 (可选)</label>
              <input
                type="text"
                value={provider.prefix || ""}
                onChange={(e) => updateProvider(type, index, "prefix", e.target.value || null)}
                placeholder="默认使用名称"
                className="w-full px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
              />
            </div>
          </div>

          <div>
            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">Base URL *</label>
            <input
              type="text"
              value={provider.base_url}
              onChange={(e) => updateProvider(type, index, "base_url", e.target.value)}
              placeholder={type === "openai" ? "https://api.deepseek.com/v1" : "https://api.example.com/v1"}
              className="w-full px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
            />
          </div>

          <div>
            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">模型列表 (逗号分隔，可选)</label>
            <input
              type="text"
              value={provider.models.join(", ")}
              onChange={(e) => updateProvider(type, index, "models", e.target.value.split(",").map(s => s.trim()).filter(s => s))}
              placeholder="deepseek-chat, deepseek-coder"
              className="w-full px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
            />
          </div>

          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="block text-xs text-gray-500 dark:text-gray-400">API Keys *</label>
              <button
                onClick={() => addApiKey(type, index)}
                className="text-xs text-blue-500 hover:text-blue-700"
              >
                + 添加 Key
              </button>
            </div>
            <div className="space-y-2">
              {provider.api_keys.map((key, keyIndex) => (
                <div key={keyIndex} className="flex gap-2">
                  <input
                    type="password"
                    value={key}
                    onChange={(e) => updateApiKey(type, index, keyIndex, e.target.value)}
                    placeholder="sk-..."
                    className="flex-1 px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
                  />
                  {provider.api_keys.length > 1 && (
                    <button
                      onClick={() => removeApiKey(type, index, keyIndex)}
                      className="px-2 text-red-500 hover:text-red-700"
                    >
                      ×
                    </button>
                  )}
                </div>
              ))}
            </div>
          </div>
        </div>
      ))}

      <button
        onClick={() => addProvider(type)}
        className="w-full py-2 border-2 border-dashed border-gray-300 dark:border-gray-600 rounded-lg text-gray-500 dark:text-gray-400 hover:border-gray-400 hover:text-gray-600 dark:hover:border-gray-500 dark:hover:text-gray-300 text-sm"
      >
        + 添加{type === "openai" ? " OpenAI 兼容" : " Claude Code 兼容"}供应商
      </button>
    </div>
  );

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <div className="w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-700 flex items-center justify-center">
          <svg className="w-5 h-5 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </div>
        <div>
          <h2 className="text-xl font-bold text-gray-800 dark:text-white">设置</h2>
          <p className="text-sm text-gray-500 dark:text-gray-400">配置应用程序选项和自定义供应商</p>
        </div>
      </div>

      {/* Tabs */}
      <div className="border-b border-gray-200 dark:border-gray-700">
        <nav className="flex gap-4">
          <button
            onClick={() => setActiveTab("general")}
            className={`py-2 px-1 border-b-2 text-sm font-medium ${activeTab === "general"
              ? "border-gray-800 dark:border-white text-gray-800 dark:text-white"
              : "border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300"
              }`}
          >
            通用设置
          </button>
          <button
            onClick={() => setActiveTab("openai")}
            className={`py-2 px-1 border-b-2 text-sm font-medium ${activeTab === "openai"
              ? "border-gray-800 dark:border-white text-gray-800 dark:text-white"
              : "border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300"
              }`}
          >
            OpenAI 兼容
            {customProviders.openai_compatibility.length > 0 && (
              <span className="ml-1 px-1.5 py-0.5 text-xs bg-gray-200 dark:bg-gray-700 rounded">
                {customProviders.openai_compatibility.length}
              </span>
            )}
          </button>
          <button
            onClick={() => setActiveTab("claude")}
            className={`py-2 px-1 border-b-2 text-sm font-medium ${activeTab === "claude"
              ? "border-gray-800 dark:border-white text-gray-800 dark:text-white"
              : "border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300"
              }`}
          >
            Claude Code 兼容
            {customProviders.claude_code_compatibility.length > 0 && (
              <span className="ml-1 px-1.5 py-0.5 text-xs bg-gray-200 dark:bg-gray-700 rounded">
                {customProviders.claude_code_compatibility.length}
              </span>
            )}
          </button>
        </nav>
      </div>

      {/* Settings Card */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6 space-y-6">
        {activeTab === "general" && (
          <>
            <div className="space-y-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                终端详细日志
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                开启后会在终端输出所有请求与响应内容（含流式片段），便于排查问题。
              </p>
              <div className="flex items-center justify-between p-3 border border-gray-300 dark:border-gray-600 rounded-lg">
                <span className="text-sm text-gray-600 dark:text-gray-300">
                  {appConfig?.debug ? "已开启" : "已关闭"}
                </span>
                <button
                  onClick={() =>
                    setAppConfig((prev) =>
                      prev ? { ...prev, debug: !prev.debug } : prev
                    )
                  }
                  className={`relative w-12 h-6 rounded-full transition-colors ${appConfig?.debug ? "bg-gray-800 dark:bg-gray-600" : "bg-gray-300 dark:bg-gray-600"
                    }`}
                >
                  <span
                    className={`absolute top-1 w-4 h-4 bg-white rounded-full transition-transform ${appConfig?.debug ? "left-7" : "left-1"
                      }`}
                  />
                </button>
              </div>
            </div>
            <div className="space-y-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                凭据存储目录
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                OAuth 登录凭据与 API Key 账号信息的保存位置。建议选择可写目录。
              </p>
              <div className="flex flex-col gap-2 md:flex-row md:items-center">
                <input
                  type="text"
                  value={appConfig?.["auth-dir"] ?? ""}
                  onChange={(e) =>
                    setAppConfig((prev) =>
                      prev ? { ...prev, "auth-dir": e.target.value } : prev
                    )
                  }
                  placeholder="例如：/Users/you/.cli-proxy-api"
                  className="flex-1 px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white text-sm"
                />
                <button
                  onClick={async () => {
                    try {
                      const selected = await openDialog({
                        directory: true,
                        multiple: false,
                      });
                      if (typeof selected === "string") {
                        setAppConfig((prev) =>
                          prev ? { ...prev, "auth-dir": selected } : prev
                        );
                      }
                    } catch (error) {
                      console.error("Failed to pick directory:", error);
                    }
                  }}
                  className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg text-sm text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700"
                >
                  选择目录
                </button>
              </div>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                修改后新登录会写入新目录，旧账号需要手动迁移或重新登录。
              </p>
            </div>
            {/* Quota Refresh Interval */}
            <div className="space-y-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                额度刷新间隔
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                设置自动刷新账号额度信息的时间间隔（刷新额度时会同时刷新 Token）
              </p>
              <select
                value={settings.quota_refresh_interval}
                onChange={(e) => setSettings({ ...settings, quota_refresh_interval: Number(e.target.value) })}
                className="mt-1 block w-full px-3 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg shadow-sm focus:outline-none focus:ring-2 focus:ring-gray-500 focus:border-gray-500 text-gray-900 dark:text-white"
              >
                {QUOTA_INTERVAL_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </div>

            {/* Account Routing Strategy */}
            <div className="space-y-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                账号轮换策略
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                选择同一供应商有多个账号时的使用策略
              </p>
              <div className="space-y-3">
                <label className="flex items-start gap-3 p-3 border border-gray-300 dark:border-gray-600 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700">
                  <input
                    type="radio"
                    name="account_routing"
                    value="stick-until-exhausted"
                    checked={settings.account_routing_strategy === "stick-until-exhausted"}
                    onChange={(e) => setSettings({ ...settings, account_routing_strategy: e.target.value })}
                    className="mt-1"
                  />
                  <div>
                    <div className="text-sm font-medium text-gray-800 dark:text-white">用完再换</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      优先使用第一个账号，直到额度耗尽或出错后自动切换到下一个账号（推荐）
                    </div>
                  </div>
                </label>
                <label className="flex items-start gap-3 p-3 border border-gray-300 dark:border-gray-600 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700">
                  <input
                    type="radio"
                    name="account_routing"
                    value="round-robin"
                    checked={settings.account_routing_strategy === "round-robin"}
                    onChange={(e) => setSettings({ ...settings, account_routing_strategy: e.target.value })}
                    className="mt-1"
                  />
                  <div>
                    <div className="text-sm font-medium text-gray-800 dark:text-white">轮询模式</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      每次请求轮换一个账号，均匀分配请求到所有账号
                    </div>
                  </div>
                </label>
              </div>
            </div>

            {/* Model Routing Mode */}
            <div className="space-y-2">
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                模型路由模式
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400">
                选择模型请求的路由方式
              </p>
              <div className="space-y-3">
                <label className="flex items-start gap-3 p-3 border border-gray-300 dark:border-gray-600 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700">
                  <input
                    type="radio"
                    name="routing_mode"
                    value="provider"
                    checked={settings.model_routing_mode === "provider"}
                    onChange={(e) => setSettings({ ...settings, model_routing_mode: e.target.value })}
                    className="mt-1"
                  />
                  <div>
                    <div className="text-sm font-medium text-gray-800 dark:text-white">供应商模式</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      需要指定供应商前缀，如 <code className="bg-gray-100 dark:bg-gray-600 px-1 rounded">kiro/claude-sonnet-4-5</code>
                    </div>
                  </div>
                </label>
                <label className="flex items-start gap-3 p-3 border border-gray-300 dark:border-gray-600 rounded-lg cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700">
                  <input
                    type="radio"
                    name="routing_mode"
                    value="model"
                    checked={settings.model_routing_mode === "model"}
                    onChange={(e) => setSettings({ ...settings, model_routing_mode: e.target.value })}
                    className="mt-1"
                  />
                  <div>
                    <div className="text-sm font-medium text-gray-800 dark:text-white">模型聚合模式</div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      按模型名称自动选择有额度的供应商，如直接使用 <code className="bg-gray-100 dark:bg-gray-600 px-1 rounded">claude-sonnet-4-5</code>
                    </div>
                  </div>
                </label>
              </div>
            </div>

            {/* Provider Priorities */}
            {settings.model_routing_mode === "model" && settings.provider_priorities.length > 0 && (
              <div className="space-y-2">
                <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                  供应商优先级
                </label>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  拖动调整优先级顺序，优先级高的供应商会先尝试（数字越大优先级越高）
                </p>
                <div className="space-y-2">
                  {settings.provider_priorities
                    .sort((a, b) => b.priority - a.priority)
                    .map((provider) => (
                      <div
                        key={provider.provider}
                        className="flex items-center gap-3 p-3 border border-gray-300 dark:border-gray-600 rounded-lg"
                      >
                        <input
                          type="checkbox"
                          checked={provider.enabled}
                          onChange={(e) => {
                            const newPriorities = settings.provider_priorities.map(p =>
                              p.provider === provider.provider ? { ...p, enabled: e.target.checked } : p
                            );
                            setSettings({ ...settings, provider_priorities: newPriorities });
                          }}
                          className="w-4 h-4"
                        />
                        <span className={`flex-1 text-sm ${provider.enabled ? 'text-gray-800 dark:text-white' : 'text-gray-400 dark:text-gray-500'}`}>
                          {provider.provider.charAt(0).toUpperCase() + provider.provider.slice(1)}
                        </span>
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => {
                              const newPriorities = settings.provider_priorities.map(p =>
                                p.provider === provider.provider ? { ...p, priority: Math.min(p.priority + 10, 200) } : p
                              );
                              setSettings({ ...settings, provider_priorities: newPriorities });
                            }}
                            className="px-2 py-1 text-xs bg-gray-100 dark:bg-gray-700 rounded hover:bg-gray-200 dark:hover:bg-gray-600"
                          >
                            ↑
                          </button>
                          <span className="text-xs text-gray-500 w-8 text-center">{provider.priority}</span>
                          <button
                            onClick={() => {
                              const newPriorities = settings.provider_priorities.map(p =>
                                p.provider === provider.provider ? { ...p, priority: Math.max(p.priority - 10, 0) } : p
                              );
                              setSettings({ ...settings, provider_priorities: newPriorities });
                            }}
                            className="px-2 py-1 text-xs bg-gray-100 dark:bg-gray-700 rounded hover:bg-gray-200 dark:hover:bg-gray-600"
                          >
                            ↓
                          </button>
                        </div>
                      </div>
                    ))}
                </div>
              </div>
            )}
          </>
        )}

        {activeTab === "openai" && (
          <div className="space-y-4">
            <div className="text-sm text-gray-600 dark:text-gray-400">
              <p>添加 OpenAI 兼容的 API 供应商（如 DeepSeek、Moonshot 等）。</p>
              <p className="mt-1">使用方式：<code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">前缀/模型名</code>，例如 <code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">deepseek/deepseek-chat</code></p>
            </div>
            {renderProviderForm("openai", customProviders.openai_compatibility)}
          </div>
        )}

        {activeTab === "claude" && (
          <div className="space-y-4">
            <div className="text-sm text-gray-600 dark:text-gray-400">
              <p>添加 Claude Code 兼容的 API 供应商（支持 Anthropic Messages API 格式）。</p>
              <p className="mt-1">使用方式：<code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">前缀/模型名</code>，例如 <code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">custom/claude-3-opus</code></p>
            </div>
            {renderProviderForm("claude", customProviders.claude_code_compatibility)}
          </div>
        )}

        {/* Save Button */}
        <div className="flex items-center gap-3 pt-4 border-t border-gray-200 dark:border-gray-700">
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-4 py-2 bg-gray-800 hover:bg-gray-900 dark:bg-gray-700 dark:hover:bg-gray-600 text-white rounded-lg font-medium disabled:opacity-50 flex items-center gap-2"
          >
            {saving ? (
              <>
                <svg className="w-4 h-4 animate-spin" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                </svg>
                保存中...
              </>
            ) : (
              "保存设置"
            )}
          </button>
          {saved && (
            <span className="text-sm text-green-600 dark:text-green-400 flex items-center gap-1">
              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
              </svg>
              已保存
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
