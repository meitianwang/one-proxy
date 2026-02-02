import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface SettingsData {
  quota_refresh_interval: number;
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
  });
  const [customProviders, setCustomProviders] = useState<CustomProvidersData>({
    openai_compatibility: [],
    claude_code_compatibility: [],
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [activeTab, setActiveTab] = useState<"general" | "openai" | "claude">("general");

  useEffect(() => {
    loadSettings();
    loadCustomProviders();
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
      await invoke("save_settings", { settings });
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
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (error) {
      console.error("Failed to save settings:", error);
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

  if (loading) {
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
            className={`py-2 px-1 border-b-2 text-sm font-medium ${
              activeTab === "general"
                ? "border-gray-800 dark:border-white text-gray-800 dark:text-white"
                : "border-transparent text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300"
            }`}
          >
            通用设置
          </button>
          <button
            onClick={() => setActiveTab("openai")}
            className={`py-2 px-1 border-b-2 text-sm font-medium ${
              activeTab === "openai"
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
            className={`py-2 px-1 border-b-2 text-sm font-medium ${
              activeTab === "claude"
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
