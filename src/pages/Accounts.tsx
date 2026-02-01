import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { ask } from "@tauri-apps/plugin-dialog";

interface AuthAccount {
  id: string;
  provider: string;
  email: string | null;
  enabled: boolean;
  prefix: string | null;
}

interface ModelQuota {
  name: string;
  percentage: number;
  reset_time: string;
}

interface QuotaData {
  models: ModelQuota[];
  last_updated: number;
  is_forbidden: boolean;
  subscription_tier: string | null;
  project_id: string | null;
}

interface CodexQuotaData {
  plan_type: string;
  primary_used: number;
  primary_resets_at: string | null;
  secondary_used: number;
  secondary_resets_at: string | null;
  has_credits: boolean;
  unlimited_credits: boolean;
  credits_balance: number | null;
  last_updated: number;
  is_error: boolean;
  error_message: string | null;
}

interface GeminiModelQuota {
  model_id: string;
  remaining_fraction: number;
  reset_time: string | null;
}

interface GeminiQuotaData {
  models: GeminiModelQuota[];
  last_updated: number;
  is_error: boolean;
  error_message: string | null;
}

const PROVIDERS = [
  { id: "google", name: "Gemini CLI", label: "Gemini", color: "bg-blue-100 text-blue-700" },
  { id: "openai", name: "Codex", label: "Codex", color: "bg-green-100 text-green-700" },
  { id: "antigravity", name: "Antigravity", label: "Antigravity", color: "bg-gray-100 text-gray-700" },
  { id: "kiro", name: "Kiro", label: "Kiro", color: "bg-orange-100 text-orange-700" },
];

// Map provider names from auth files to display info
const PROVIDER_ALIASES: Record<string, string> = {
  "gemini": "google",
  "codex": "openai",
};

function getProviderInfo(provider: string) {
  const normalizedProvider = PROVIDER_ALIASES[provider] || provider;
  return PROVIDERS.find((p) => p.id === normalizedProvider) || { label: provider, color: "bg-gray-100 text-gray-700" };
}

export function Accounts() {
  const [accounts, setAccounts] = useState<AuthAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [loginInProgress, setLoginInProgress] = useState<string | null>(null);
  const [showProjectPrompt, setShowProjectPrompt] = useState(false);
  const [projectIdInput, setProjectIdInput] = useState("");
  const [pendingGeminiAccountId, setPendingGeminiAccountId] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [showAddMenu, setShowAddMenu] = useState(false);
  const [viewMode, setViewMode] = useState<"list" | "card">("list");
  const [quotaData, setQuotaData] = useState<Record<string, QuotaData>>({});
  const [quotaLoading, setQuotaLoading] = useState<Record<string, boolean>>({});
  const [codexQuotaData, setCodexQuotaData] = useState<Record<string, CodexQuotaData>>({});
  const [codexQuotaLoading, setCodexQuotaLoading] = useState<Record<string, boolean>>({});
  const [geminiQuotaData, setGeminiQuotaData] = useState<Record<string, GeminiQuotaData>>({});
  const [geminiQuotaLoading, setGeminiQuotaLoading] = useState<Record<string, boolean>>({});
  const addMenuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (addMenuRef.current && !addMenuRef.current.contains(event.target as Node)) {
        setShowAddMenu(false);
      }
    }
    if (showAddMenu) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [showAddMenu]);

  useEffect(() => {
    fetchAccounts();

    // Poll for account changes when login is in progress
    let interval: ReturnType<typeof setInterval> | null = null;
    if (loginInProgress) {
      interval = setInterval(fetchAccounts, 2000);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [loginInProgress]);

  // Auto-fetch quota for Antigravity accounts when accounts change
  useEffect(() => {
    const antigravityAccounts = accounts.filter(a => a.provider === "antigravity");
    for (const account of antigravityAccounts) {
      if (!quotaData[account.id] && !quotaLoading[account.id]) {
        fetchQuota(account.id);
      }
    }
  }, [accounts]);

  // Auto-fetch quota for Codex accounts when accounts change
  useEffect(() => {
    const codexAccounts = accounts.filter(a => a.provider === "openai" || a.provider === "codex");
    for (const account of codexAccounts) {
      if (!codexQuotaData[account.id] && !codexQuotaLoading[account.id]) {
        fetchCodexQuota(account.id);
      }
    }
  }, [accounts]);

  // Auto-fetch quota for Gemini accounts when accounts change
  useEffect(() => {
    const geminiAccounts = accounts.filter(a => a.provider === "gemini" || a.provider === "google");
    for (const account of geminiAccounts) {
      if (!geminiQuotaData[account.id] && !geminiQuotaLoading[account.id]) {
        fetchGeminiQuota(account.id);
      }
    }
  }, [accounts]);

  async function fetchAccounts() {
    try {
      setLoading(true);
      const result = await invoke<AuthAccount[]>("get_auth_accounts");
      console.log("Fetched accounts:", result);
      console.log("Account count:", result.length);

      // If we were waiting for login and got new accounts, stop polling
      if (loginInProgress && result.length > accounts.length) {
        setLoginInProgress(null);
      }

      setAccounts(result);
    } catch (error) {
      console.error("Failed to fetch accounts:", error);
    } finally {
      setLoading(false);
    }
  }

  async function refreshAllQuotas() {
    const antigravityAccounts = accounts.filter(a => a.provider === "antigravity");
    for (const account of antigravityAccounts) {
      fetchQuota(account.id);
    }
    const codexAccounts = accounts.filter(a => a.provider === "openai" || a.provider === "codex");
    for (const account of codexAccounts) {
      fetchCodexQuota(account.id);
    }
    const geminiAccounts = accounts.filter(a => a.provider === "gemini" || a.provider === "google");
    for (const account of geminiAccounts) {
      fetchGeminiQuota(account.id);
    }
  }

  async function handleRefresh() {
    await fetchAccounts();
    refreshAllQuotas();
  }

  async function startLogin(provider: string) {
    try {
      console.log("Starting OAuth login for provider:", provider);
      setLoginInProgress(provider);

      // Check if server is running, start it if not
      console.log("Checking server status...");
      const status = await invoke<{ running: boolean }>("get_server_status");
      console.log("Server status:", status);

      if (!status.running) {
        console.log("Server not running, starting it first...");
        await invoke("start_server");
        // Wait a bit for server to start
        await new Promise(resolve => setTimeout(resolve, 500));
      }

      console.log("Calling start_oauth_login...");
      const authUrl = await invoke<string>("start_oauth_login", { provider });
      console.log("Got auth URL:", authUrl);

      // For OpenAI/Codex, the backend handles the entire OAuth flow including opening the browser
      // and returns a success message instead of a URL
      if (authUrl && authUrl.startsWith("http")) {
        console.log("Opening URL in browser...");
        await open(authUrl);
        console.log("Browser opened successfully");
      } else if (authUrl) {
        // OAuth completed successfully (e.g., OpenAI flow)
        console.log("OAuth completed:", authUrl);
        const beforeIds = new Set(accounts.map((account) => account.id));
        const latestAccounts = await invoke<AuthAccount[]>("get_auth_accounts");
        setAccounts(latestAccounts);
        if (provider === "google") {
          const newGemini = latestAccounts.find(
            (account) =>
              account.provider === "gemini" && !beforeIds.has(account.id)
          );
          const fallbackGemini = latestAccounts.find(
            (account) => account.provider === "gemini"
          );
          const target = newGemini ?? fallbackGemini;
          if (target) {
            setPendingGeminiAccountId(target.id);
            setProjectIdInput("");
            setShowProjectPrompt(true);
          }
        }
        setLoginInProgress(null);
        return;
      } else {
        console.error("No auth URL returned");
        alert("登录失败: 未返回授权 URL");
        setLoginInProgress(null);
      }

      // Auto-clear login state after 60 seconds
      setTimeout(() => setLoginInProgress(null), 60000);
    } catch (error) {
      console.error("Failed to start OAuth:", error);
      alert(`登录失败: ${error}`);
      setLoginInProgress(null);
    }
  }

  async function handleLogin(provider: string) {
    await startLogin(provider);
  }

  async function handleProjectConfirm() {
    const trimmed = projectIdInput.trim();
    if (!trimmed) {
      alert("需要填写项目 ID 才能继续登录");
      return;
    }
    setShowProjectPrompt(false);
    const accountId = pendingGeminiAccountId;
    setPendingGeminiAccountId(null);
    if (!accountId) {
      alert("未找到可更新的 Gemini 账户");
      return;
    }
    try {
      await invoke("set_gemini_project_id", {
        accountId,
        projectId: trimmed,
      });
      alert("项目 ID 已保存");
    } catch (error) {
      console.error("Failed to save project id:", error);
      alert(`保存失败: ${error}`);
    }
  }

  function handleProjectCancel() {
    setShowProjectPrompt(false);
    setPendingGeminiAccountId(null);
    setProjectIdInput("");
    setLoginInProgress(null);
  }

  async function handleDelete(accountId: string) {
    const confirmed = await ask("确定要删除此账户吗？", {
      title: "删除确认",
      kind: "warning",
    });

    if (!confirmed) {
      return;
    }

    try {
      console.log("Deleting account:", accountId);
      await invoke("delete_account", { accountId: accountId });
      console.log("Account deleted successfully");
      await fetchAccounts();
    } catch (error) {
      console.error("Failed to delete account:", error);
      alert(`删除失败: ${error}`);
    }
  }

  async function handleToggleEnabled(accountId: string, enabled: boolean) {
    try {
      await invoke("set_account_enabled", { accountId, enabled });
      await fetchAccounts();
    } catch (error) {
      console.error("Failed to toggle account:", error);
      alert(`操作失败: ${error}`);
    }
  }

  async function fetchQuota(accountId: string) {
    if (quotaLoading[accountId]) return;
    setQuotaLoading((prev) => ({ ...prev, [accountId]: true }));
    try {
      const result = await invoke<QuotaData>("fetch_antigravity_quota", { accountId });
      setQuotaData((prev) => ({ ...prev, [accountId]: result }));
    } catch (error) {
      console.error("Failed to fetch quota:", error);
    } finally {
      setQuotaLoading((prev) => ({ ...prev, [accountId]: false }));
    }
  }

  async function fetchCodexQuota(accountId: string) {
    if (codexQuotaLoading[accountId]) return;
    setCodexQuotaLoading((prev) => ({ ...prev, [accountId]: true }));
    try {
      const result = await invoke<CodexQuotaData>("fetch_codex_quota", { accountId });
      setCodexQuotaData((prev) => ({ ...prev, [accountId]: result }));
    } catch (error) {
      console.error("Failed to fetch codex quota:", error);
    } finally {
      setCodexQuotaLoading((prev) => ({ ...prev, [accountId]: false }));
    }
  }

  async function fetchGeminiQuota(accountId: string) {
    if (geminiQuotaLoading[accountId]) return;
    setGeminiQuotaLoading((prev) => ({ ...prev, [accountId]: true }));
    try {
      const result = await invoke<GeminiQuotaData>("fetch_gemini_quota", { accountId });
      setGeminiQuotaData((prev) => ({ ...prev, [accountId]: result }));
    } catch (error) {
      console.error("Failed to fetch gemini quota:", error);
    } finally {
      setGeminiQuotaLoading((prev) => ({ ...prev, [accountId]: false }));
    }
  }

  function getQuotaColor(percentage: number): string {
    if (percentage >= 50) return "bg-emerald-500";
    if (percentage >= 20) return "bg-amber-500";
    return "bg-red-500";
  }

  function getQuotaTextColor(percentage: number): string {
    if (percentage >= 50) return "text-emerald-600 dark:text-emerald-400";
    if (percentage >= 20) return "text-amber-600 dark:text-amber-400";
    return "text-red-600 dark:text-red-400";
  }

  // For Codex, the API returns "used" percentage, so lower remaining = worse
  function getCodexUsageColor(usedPercent: number): string {
    const remaining = 100 - usedPercent;
    if (remaining >= 50) return "text-emerald-600 dark:text-emerald-400";
    if (remaining >= 20) return "text-amber-600 dark:text-amber-400";
    return "text-red-600 dark:text-red-400";
  }

  function formatResetTime(resetTime: string): string {
    if (!resetTime) return "";
    try {
      const date = new Date(resetTime);
      const now = new Date();
      const diffMs = date.getTime() - now.getTime();
      if (diffMs <= 0) return "已重置";
      const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
      const diffMins = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));
      if (diffHours > 0) return `${diffHours}h ${diffMins}m`;
      return `${diffMins}m`;
    } catch {
      return "";
    }
  }

  function getModelDisplayName(name: string): string {
    if (name === "gemini-3-pro-high") return "G3 Pro";
    if (name === "gemini-3-flash") return "G3 Flash";
    if (name === "gemini-3-pro-image") return "G3 Image";
    if (name === "claude-sonnet-4-5-thinking") return "Claude";
    // Gemini CLI models
    if (name === "gemini-2.5-pro") return "2.5 Pro";
    if (name === "gemini-2.5-flash") return "2.5 Flash";
    if (name === "gemini-2.5-flash-lite") return "2.5 Lite";
    if (name === "gemini-3-pro-preview") return "3 Pro";
    if (name === "gemini-3-flash-preview") return "3 Flash";
    if (name === "gemini-2.0-flash") return "2.0 Flash";
    return name.split("/").pop() || name;
  }

  // Get key Gemini models for display
  function getKeyGeminiModels(models: GeminiModelQuota[]): GeminiModelQuota[] {
    const keyModelNames = [
      "gemini-2.5-pro",
      "gemini-2.5-flash",
      "gemini-3-pro-preview",
      "gemini-3-flash-preview"
    ];

    return keyModelNames
      .map(name => models.find(m => m.model_id === name))
      .filter((m): m is GeminiModelQuota => m !== undefined);
  }

  // Get the 4 key models for display
  function getKeyModels(models: ModelQuota[]): ModelQuota[] {
    const keyModelNames = [
      "gemini-3-pro-high",
      "gemini-3-flash",
      "gemini-3-pro-image",
      "claude-sonnet-4-5-thinking"
    ];

    return keyModelNames
      .map(name => models.find(m => m.name === name))
      .filter((m): m is ModelQuota => m !== undefined);
  }

  async function handleBatchEnable() {
    if (selectedIds.size === 0) return;
    try {
      for (const id of selectedIds) {
        await invoke("set_account_enabled", { accountId: id, enabled: true });
      }
      await fetchAccounts();
      setSelectedIds(new Set());
    } catch (error) {
      console.error("Failed to enable accounts:", error);
      alert(`批量启用失败: ${error}`);
    }
  }

  async function handleBatchDisable() {
    if (selectedIds.size === 0) return;
    try {
      for (const id of selectedIds) {
        await invoke("set_account_enabled", { accountId: id, enabled: false });
      }
      await fetchAccounts();
      setSelectedIds(new Set());
    } catch (error) {
      console.error("Failed to disable accounts:", error);
      alert(`批量禁用失败: ${error}`);
    }
  }

  const filteredAccounts = accounts;

  const allSelected = filteredAccounts.length > 0 && filteredAccounts.every((a) => selectedIds.has(a.id));

  function toggleSelectAll() {
    if (allSelected) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(filteredAccounts.map((a) => a.id)));
    }
  }

  function toggleSelect(id: string) {
    const newSet = new Set(selectedIds);
    if (newSet.has(id)) {
      newSet.delete(id);
    } else {
      newSet.add(id);
    }
    setSelectedIds(newSet);
  }

  return (
    <>
      <div className="space-y-4">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-emerald-100 flex items-center justify-center">
              <svg className="w-5 h-5 text-emerald-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197m13.5-9a2.5 2.5 0 11-5 0 2.5 2.5 0 015 0z" />
              </svg>
            </div>
            <div>
              <h2 className="text-xl font-bold text-gray-800 dark:text-white">账号管理</h2>
              <p className="text-sm text-gray-500 dark:text-gray-400">管理你的 OAuth 账号</p>
            </div>
          </div>

          {/* Toolbar */}
          <div className="flex items-center gap-2">
            {/* Add Account Dropdown */}
            <div className="relative" ref={addMenuRef}>
              <button
                onClick={() => setShowAddMenu(!showAddMenu)}
                disabled={loginInProgress !== null}
                className="px-4 py-2 rounded-lg bg-emerald-500 hover:bg-emerald-600 text-white text-sm font-medium flex items-center gap-2 disabled:opacity-50"
              >
                <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
                </svg>
                添加账号
              </button>
              {showAddMenu && (
                <div className="absolute right-0 mt-2 w-48 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-10">
                  {PROVIDERS.map((provider) => (
                    <button
                      key={provider.id}
                      onClick={() => {
                        setShowAddMenu(false);
                        handleLogin(provider.id);
                      }}
                      className="w-full px-4 py-2 text-left text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700"
                    >
                      {provider.name}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {/* Refresh */}
            <button
              onClick={handleRefresh}
              className="p-2 rounded-lg border border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700"
              title="刷新账号和额度"
            >
              <svg className="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
              </svg>
            </button>

            {/* View Mode Toggle */}
            <div className="flex border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
              <button
                onClick={() => setViewMode("list")}
                className={`p-2 ${viewMode === "list" ? "bg-emerald-100 dark:bg-emerald-900/30 text-emerald-600" : "hover:bg-gray-50 dark:hover:bg-gray-700 text-gray-500"}`}
                title="列表视图"
              >
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16M4 10h16M4 14h16M4 18h16" />
                </svg>
              </button>
              <button
                onClick={() => setViewMode("card")}
                className={`p-2 ${viewMode === "card" ? "bg-emerald-100 dark:bg-emerald-900/30 text-emerald-600" : "hover:bg-gray-50 dark:hover:bg-gray-700 text-gray-500"}`}
                title="卡片视图"
              >
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        {/* Select All & Count */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <label className="flex items-center gap-2 cursor-pointer">
              <input
                type="checkbox"
                checked={allSelected}
                onChange={toggleSelectAll}
                className="w-4 h-4 rounded border-gray-300 text-emerald-500 focus:ring-emerald-500"
              />
              <span className="text-sm text-emerald-600 dark:text-emerald-400">全选</span>
            </label>
            {selectedIds.size > 0 && (
              <div className="flex items-center gap-2">
                <button
                  onClick={handleBatchEnable}
                  className="px-3 py-1 text-xs rounded-lg bg-emerald-100 text-emerald-700 hover:bg-emerald-200 dark:bg-emerald-900/30 dark:text-emerald-400 dark:hover:bg-emerald-900/50"
                >
                  批量启用 ({selectedIds.size})
                </button>
                <button
                  onClick={handleBatchDisable}
                  className="px-3 py-1 text-xs rounded-lg bg-gray-100 text-gray-700 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600"
                >
                  批量禁用 ({selectedIds.size})
                </button>
              </div>
            )}
          </div>
          <span className="text-sm text-emerald-600 dark:text-emerald-400">
            共 {filteredAccounts.length} 个账号
          </span>
        </div>

        {/* Table View */}
        {viewMode === "list" && (
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow overflow-x-auto">
            <table className="w-full min-w-[600px]">
              <thead className="bg-emerald-50 dark:bg-emerald-900/20">
                <tr>
                  <th className="w-10 px-3 py-3"></th>
                  <th className="w-56 px-3 py-3 text-left text-sm font-medium text-gray-600 dark:text-gray-400 whitespace-nowrap">邮箱</th>
                  <th className="px-3 py-3 text-left text-sm font-medium text-gray-600 dark:text-gray-400 whitespace-nowrap">模型配额</th>
                  <th className="px-3 py-3 text-left text-sm font-medium text-gray-600 dark:text-gray-400 whitespace-nowrap">状态</th>
                  <th className="px-3 py-3 text-left text-sm font-medium text-gray-600 dark:text-gray-400 whitespace-nowrap">操作</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100 dark:divide-gray-700">
                {loading ? (
                  <tr>
                    <td colSpan={5} className="px-3 py-8 text-center text-gray-500 dark:text-gray-400">
                      加载中...
                    </td>
                  </tr>
                ) : filteredAccounts.length === 0 ? (
                  <tr>
                    <td colSpan={5} className="px-3 py-8 text-center text-gray-500 dark:text-gray-400">
                      暂无账号，请点击「添加账号」按钮
                    </td>
                  </tr>
                ) : (
                  filteredAccounts.map((account) => {
                    const providerInfo = getProviderInfo(account.provider);
                    const isAntigravity = account.provider === "antigravity";
                    const isCodex = account.provider === "openai" || account.provider === "codex";
                    const isGemini = account.provider === "gemini" || account.provider === "google";
                    const quota = quotaData[account.id];
                    const isLoadingQuota = quotaLoading[account.id];
                    const codexQuota = codexQuotaData[account.id];
                    const isLoadingCodexQuota = codexQuotaLoading[account.id];
                    const geminiQuota = geminiQuotaData[account.id];
                    const isLoadingGeminiQuota = geminiQuotaLoading[account.id];
                    return (
                      <tr key={account.id} className="hover:bg-gray-50 dark:hover:bg-gray-700/50">
                        <td className="px-3 py-3">
                          <input
                            type="checkbox"
                            checked={selectedIds.has(account.id)}
                            onChange={() => toggleSelect(account.id)}
                            className="w-4 h-4 rounded border-gray-300 text-emerald-500 focus:ring-emerald-500"
                          />
                        </td>
                        <td className="px-3 py-3 w-56 max-w-56">
                          <div className="flex items-center gap-2">
                            <span className="text-sm font-medium text-gray-800 dark:text-white truncate max-w-[120px]" title={account.email || account.id}>
                              {account.email || account.id}
                            </span>
                            <span className={`inline-block px-2 py-0.5 text-xs rounded border flex-shrink-0 ${providerInfo.color}`}>
                              {providerInfo.label}
                            </span>
                            {isAntigravity && quota?.subscription_tier && (
                              <span className={`inline-block px-1.5 py-0.5 text-[10px] rounded font-medium flex-shrink-0 ${
                                quota.subscription_tier.toLowerCase().includes("pro")
                                  ? "bg-blue-500 text-white"
                                  : quota.subscription_tier.toLowerCase().includes("ultra")
                                  ? "bg-purple-500 text-white"
                                  : "bg-gray-400 text-white"
                              }`}>
                                {quota.subscription_tier.toLowerCase().includes("pro") ? "PRO" :
                                 quota.subscription_tier.toLowerCase().includes("ultra") ? "ULTRA" : "FREE"}
                              </span>
                            )}
                            {isCodex && codexQuota && !codexQuota.is_error && (
                              <span className={`inline-block px-1.5 py-0.5 text-[10px] rounded font-medium flex-shrink-0 ${
                                codexQuota.plan_type.toLowerCase().includes("plus")
                                  ? "bg-green-500 text-white"
                                  : "bg-gray-400 text-white"
                              }`}>
                                {codexQuota.plan_type.toUpperCase()}
                              </span>
                            )}
                          </div>
                        </td>
                        <td className="px-3 py-3">
                          {isAntigravity ? (
                            isLoadingQuota ? (
                              <span className="text-xs text-gray-400">加载中...</span>
                            ) : quota?.is_forbidden ? (
                              <span className="text-xs text-red-500">已禁用</span>
                            ) : quota ? (
                              <div className="grid grid-cols-2 gap-x-4 gap-y-1">
                                {getKeyModels(quota.models).map((model) => (
                                  <div key={model.name} className="flex items-center gap-1 text-xs whitespace-nowrap">
                                    <span className="text-gray-600 dark:text-gray-400 font-medium w-16">{getModelDisplayName(model.name)}</span>
                                    {model.reset_time && (
                                      <span className="text-gray-400 flex items-center gap-0.5">
                                        <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                        </svg>
                                        {formatResetTime(model.reset_time)}
                                      </span>
                                    )}
                                    <span className={`font-bold ${getQuotaTextColor(model.percentage)}`}>
                                      {model.percentage}%
                                    </span>
                                  </div>
                                ))}
                              </div>
                            ) : (
                              <span className="text-xs text-gray-400">-</span>
                            )
                          ) : isCodex ? (
                            isLoadingCodexQuota ? (
                              <span className="text-xs text-gray-400">加载中...</span>
                            ) : codexQuota?.is_error ? (
                              <span className="text-xs text-red-500">{codexQuota.error_message || "错误"}</span>
                            ) : codexQuota ? (
                              <div className="grid grid-cols-2 gap-x-4 gap-y-1">
                                <div className="flex items-center gap-1 text-xs whitespace-nowrap">
                                  <span className="text-gray-600 dark:text-gray-400 font-medium w-16">5小时</span>
                                  {codexQuota.primary_resets_at && (
                                    <span className="text-gray-400 flex items-center gap-0.5">
                                      <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                      </svg>
                                      {formatResetTime(codexQuota.primary_resets_at)}
                                    </span>
                                  )}
                                  <span className={`font-bold ${getCodexUsageColor(codexQuota.primary_used)}`}>
                                    {(100 - codexQuota.primary_used).toFixed(0)}%
                                  </span>
                                </div>
                                <div className="flex items-center gap-1 text-xs whitespace-nowrap">
                                  <span className="text-gray-600 dark:text-gray-400 font-medium w-16">周限制</span>
                                  {codexQuota.secondary_resets_at && (
                                    <span className="text-gray-400 flex items-center gap-0.5">
                                      <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                      </svg>
                                      {formatResetTime(codexQuota.secondary_resets_at)}
                                    </span>
                                  )}
                                  <span className={`font-bold ${getCodexUsageColor(codexQuota.secondary_used)}`}>
                                    {(100 - codexQuota.secondary_used).toFixed(0)}%
                                  </span>
                                </div>
                              </div>
                            ) : (
                              <span className="text-xs text-gray-400">-</span>
                            )
                          ) : isGemini ? (
                            isLoadingGeminiQuota ? (
                              <span className="text-xs text-gray-400">加载中...</span>
                            ) : geminiQuota?.is_error ? (
                              <span className="text-xs text-red-500">{geminiQuota.error_message || "错误"}</span>
                            ) : geminiQuota ? (
                              <div className="grid grid-cols-2 gap-x-4 gap-y-1">
                                {getKeyGeminiModels(geminiQuota.models).map((model) => (
                                  <div key={model.model_id} className="flex items-center gap-1 text-xs whitespace-nowrap">
                                    <span className="text-gray-600 dark:text-gray-400 font-medium w-16">{getModelDisplayName(model.model_id)}</span>
                                    {model.reset_time && (
                                      <span className="text-gray-400 flex items-center gap-0.5">
                                        <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                        </svg>
                                        {formatResetTime(model.reset_time)}
                                      </span>
                                    )}
                                    <span className={`font-bold ${getQuotaTextColor(Math.round(model.remaining_fraction * 100))}`}>
                                      {Math.round(model.remaining_fraction * 100)}%
                                    </span>
                                  </div>
                                ))}
                              </div>
                            ) : (
                              <span className="text-xs text-gray-400">-</span>
                            )
                          ) : (
                            <span className="text-xs text-gray-400">-</span>
                          )}
                        </td>
                        <td className="px-3 py-3">
                          <span className={`inline-block px-2 py-1 text-xs rounded ${
                            account.enabled
                              ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
                              : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                          }`}>
                            {account.enabled ? "正常" : "禁用"}
                          </span>
                        </td>
                        <td className="px-3 py-3">
                          <div className="flex items-center gap-1">
                            {isAntigravity && (
                              <button
                                onClick={() => fetchQuota(account.id)}
                                disabled={isLoadingQuota}
                                className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                                title="刷新额度"
                              >
                                <svg className={`w-4 h-4 ${isLoadingQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                                </svg>
                              </button>
                            )}
                            {isCodex && (
                              <button
                                onClick={() => fetchCodexQuota(account.id)}
                                disabled={isLoadingCodexQuota}
                                className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingCodexQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                                title="刷新额度"
                              >
                                <svg className={`w-4 h-4 ${isLoadingCodexQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                                </svg>
                              </button>
                            )}
                            {isGemini && (
                              <button
                                onClick={() => fetchGeminiQuota(account.id)}
                                disabled={isLoadingGeminiQuota}
                                className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingGeminiQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                                title="刷新额度"
                              >
                                <svg className={`w-4 h-4 ${isLoadingGeminiQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                                </svg>
                              </button>
                            )}
                            <button
                              onClick={() => handleToggleEnabled(account.id, !account.enabled)}
                              className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${
                                account.enabled
                                  ? "text-gray-400 hover:text-orange-500"
                                  : "text-emerald-500 hover:text-emerald-600"
                              }`}
                              title={account.enabled ? "禁用账号" : "启用账号"}
                            >
                              {account.enabled ? (
                                <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
                                </svg>
                              ) : (
                                <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                                </svg>
                              )}
                            </button>
                            <button
                              onClick={() => handleDelete(account.id)}
                              className="p-1.5 rounded text-gray-400 hover:text-red-500 hover:bg-gray-100 dark:hover:bg-gray-700"
                              title="删除账号"
                            >
                              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                              </svg>
                            </button>
                          </div>
                        </td>
                      </tr>
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
        )}

        {/* Card View */}
        {viewMode === "card" && (
          <div>
            {loading ? (
              <div className="text-center py-8 text-gray-500 dark:text-gray-400">加载中...</div>
            ) : filteredAccounts.length === 0 ? (
              <div className="text-center py-8 text-gray-500 dark:text-gray-400">
                暂无账号，请点击「添加账号」按钮
              </div>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {filteredAccounts.map((account) => {
                  const providerInfo = getProviderInfo(account.provider);
                  const isAntigravity = account.provider === "antigravity";
                  const isCodex = account.provider === "openai" || account.provider === "codex";
                  const isGemini = account.provider === "gemini" || account.provider === "google";
                  const quota = quotaData[account.id];
                  const isLoadingQuota = quotaLoading[account.id];
                  const codexQuota = codexQuotaData[account.id];
                  const isLoadingCodexQuota = codexQuotaLoading[account.id];
                  const geminiQuota = geminiQuotaData[account.id];
                  const isLoadingGeminiQuota = geminiQuotaLoading[account.id];
                  return (
                    <div
                      key={account.id}
                      className={`bg-white dark:bg-gray-800 rounded-lg shadow p-4 border-2 transition-colors ${
                        selectedIds.has(account.id)
                          ? "border-emerald-500"
                          : "border-transparent hover:border-gray-200 dark:hover:border-gray-700"
                      }`}
                    >
                      <div className="flex items-start justify-between">
                        <div className="flex items-center gap-3">
                          <input
                            type="checkbox"
                            checked={selectedIds.has(account.id)}
                            onChange={() => toggleSelect(account.id)}
                            className="w-4 h-4 rounded border-gray-300 text-emerald-500 focus:ring-emerald-500"
                          />
                        </div>
                        <div className="flex items-center gap-2">
                          {isAntigravity && quota?.subscription_tier && (
                            <span className={`inline-block px-2 py-1 text-xs rounded font-medium ${
                              quota.subscription_tier.toLowerCase().includes("pro")
                                ? "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400"
                                : quota.subscription_tier.toLowerCase().includes("ultra")
                                ? "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400"
                                : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                            }`}>
                              {quota.subscription_tier.toLowerCase().includes("pro") ? "PRO" :
                               quota.subscription_tier.toLowerCase().includes("ultra") ? "ULTRA" : "FREE"}
                            </span>
                          )}
                          {isCodex && codexQuota && !codexQuota.is_error && (
                            <span className={`inline-block px-2 py-1 text-xs rounded font-medium ${
                              codexQuota.plan_type.toLowerCase().includes("plus")
                                ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400"
                                : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                            }`}>
                              {codexQuota.plan_type.toUpperCase()}
                            </span>
                          )}
                          <span className={`inline-block px-2 py-1 text-xs rounded border ${providerInfo.color}`}>
                            {providerInfo.label}
                          </span>
                        </div>
                      </div>
                      <div className="mt-3">
                        <p className="text-sm font-medium text-gray-800 dark:text-white truncate">
                          {account.email || account.id}
                        </p>
                      </div>

                      {/* Quota Display for Antigravity */}
                      {isAntigravity && (
                        <div className="mt-3 min-h-[140px]">
                          {isLoadingQuota && (
                            <p className="text-xs text-gray-500 dark:text-gray-400">加载额度中...</p>
                          )}
                          {quota && !isLoadingQuota && (
                            <div className="space-y-2">
                              {quota.is_forbidden ? (
                                <p className="text-xs text-red-500">账号已被禁用 (403)</p>
                              ) : (
                                <div className="grid grid-cols-2 gap-2">
                                  {getKeyModels(quota.models).map((model) => (
                                    <div key={model.name} className="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                                      <div className="flex items-center justify-between mb-1">
                                        <span className="text-xs font-medium text-gray-700 dark:text-gray-300 truncate">
                                          {getModelDisplayName(model.name)}
                                        </span>
                                        <span className={`text-xs font-bold ${getQuotaTextColor(model.percentage)}`}>
                                          {model.percentage}%
                                        </span>
                                      </div>
                                      <div className="w-full h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                        <div
                                          className={`h-full ${getQuotaColor(model.percentage)} transition-all`}
                                          style={{ width: `${model.percentage}%` }}
                                        />
                                      </div>
                                      {model.reset_time && (
                                        <div className="flex items-center gap-1 mt-1">
                                          <svg className="w-3 h-3 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                          </svg>
                                          <span className="text-xs text-gray-500 dark:text-gray-400">
                                            {formatResetTime(model.reset_time)}
                                          </span>
                                        </div>
                                      )}
                                    </div>
                                  ))}
                                  {getKeyModels(quota.models).length === 0 && (
                                    <p className="col-span-2 text-xs text-gray-500 dark:text-gray-400">无可用模型</p>
                                  )}
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      )}

                      {/* Quota Display for Codex */}
                      {isCodex && (
                        <div className="mt-3 min-h-[80px]">
                          {isLoadingCodexQuota && (
                            <p className="text-xs text-gray-500 dark:text-gray-400">加载额度中...</p>
                          )}
                          {codexQuota && !isLoadingCodexQuota && (
                            <div className="space-y-2">
                              {codexQuota.is_error ? (
                                <p className="text-xs text-red-500">{codexQuota.error_message || "获取额度失败"}</p>
                              ) : (
                                <div className="grid grid-cols-2 gap-2">
                                  <div className="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                                    <div className="flex items-center justify-between mb-1">
                                      <span className="text-xs font-medium text-gray-700 dark:text-gray-300">5小时</span>
                                      <span className={`text-xs font-bold ${getCodexUsageColor(codexQuota.primary_used)}`}>
                                        {(100 - codexQuota.primary_used).toFixed(0)}%
                                      </span>
                                    </div>
                                    <div className="w-full h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                      <div
                                        className={`h-full ${getQuotaColor(100 - codexQuota.primary_used)} transition-all`}
                                        style={{ width: `${100 - codexQuota.primary_used}%` }}
                                      />
                                    </div>
                                    {codexQuota.primary_resets_at && (
                                      <div className="flex items-center gap-1 mt-1">
                                        <svg className="w-3 h-3 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                        </svg>
                                        <span className="text-xs text-gray-500 dark:text-gray-400">
                                          {formatResetTime(codexQuota.primary_resets_at)}
                                        </span>
                                      </div>
                                    )}
                                  </div>
                                  <div className="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                                    <div className="flex items-center justify-between mb-1">
                                      <span className="text-xs font-medium text-gray-700 dark:text-gray-300">周限制</span>
                                      <span className={`text-xs font-bold ${getCodexUsageColor(codexQuota.secondary_used)}`}>
                                        {(100 - codexQuota.secondary_used).toFixed(0)}%
                                      </span>
                                    </div>
                                    <div className="w-full h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                      <div
                                        className={`h-full ${getQuotaColor(100 - codexQuota.secondary_used)} transition-all`}
                                        style={{ width: `${100 - codexQuota.secondary_used}%` }}
                                      />
                                    </div>
                                    {codexQuota.secondary_resets_at && (
                                      <div className="flex items-center gap-1 mt-1">
                                        <svg className="w-3 h-3 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                        </svg>
                                        <span className="text-xs text-gray-500 dark:text-gray-400">
                                          {formatResetTime(codexQuota.secondary_resets_at)}
                                        </span>
                                      </div>
                                    )}
                                  </div>
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      )}

                      {/* Quota Display for Gemini */}
                      {isGemini && (
                        <div className="mt-3 min-h-[80px]">
                          {isLoadingGeminiQuota && (
                            <p className="text-xs text-gray-500 dark:text-gray-400">加载额度中...</p>
                          )}
                          {geminiQuota && !isLoadingGeminiQuota && (
                            <div className="space-y-2">
                              {geminiQuota.is_error ? (
                                <p className="text-xs text-red-500">{geminiQuota.error_message || "获取额度失败"}</p>
                              ) : (
                                <div className="grid grid-cols-2 gap-2">
                                  {getKeyGeminiModels(geminiQuota.models).map((model) => (
                                    <div key={model.model_id} className="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                                      <div className="flex items-center justify-between mb-1">
                                        <span className="text-xs font-medium text-gray-700 dark:text-gray-300 truncate">
                                          {getModelDisplayName(model.model_id)}
                                        </span>
                                        <span className={`text-xs font-bold ${getQuotaTextColor(Math.round(model.remaining_fraction * 100))}`}>
                                          {Math.round(model.remaining_fraction * 100)}%
                                        </span>
                                      </div>
                                      <div className="w-full h-1.5 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                                        <div
                                          className={`h-full ${getQuotaColor(Math.round(model.remaining_fraction * 100))} transition-all`}
                                          style={{ width: `${model.remaining_fraction * 100}%` }}
                                        />
                                      </div>
                                      {model.reset_time && (
                                        <div className="flex items-center gap-1 mt-1">
                                          <svg className="w-3 h-3 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                                          </svg>
                                          <span className="text-xs text-gray-500 dark:text-gray-400">
                                            {formatResetTime(model.reset_time)}
                                          </span>
                                        </div>
                                      )}
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>
                          )}
                        </div>
                      )}

                      <div className="mt-4 flex items-center justify-between">
                        <span className={`inline-block px-2 py-1 text-xs rounded ${
                          account.enabled
                            ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300"
                            : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                        }`}>
                          {account.enabled ? "正常" : "禁用"}
                        </span>
                        <div className="flex items-center gap-1">
                          {isAntigravity && (
                            <button
                              onClick={() => fetchQuota(account.id)}
                              disabled={isLoadingQuota}
                              className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                              title="刷新额度"
                            >
                              <svg className={`w-4 h-4 ${isLoadingQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                              </svg>
                            </button>
                          )}
                          {isCodex && (
                            <button
                              onClick={() => fetchCodexQuota(account.id)}
                              disabled={isLoadingCodexQuota}
                              className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingCodexQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                              title="刷新额度"
                            >
                              <svg className={`w-4 h-4 ${isLoadingCodexQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                              </svg>
                            </button>
                          )}
                          {isGemini && (
                            <button
                              onClick={() => fetchGeminiQuota(account.id)}
                              disabled={isLoadingGeminiQuota}
                              className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${isLoadingGeminiQuota ? "text-gray-300" : "text-gray-400 hover:text-emerald-500"}`}
                              title="刷新额度"
                            >
                              <svg className={`w-4 h-4 ${isLoadingGeminiQuota ? "animate-spin" : ""}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                              </svg>
                            </button>
                          )}
                          <button
                            onClick={() => handleToggleEnabled(account.id, !account.enabled)}
                            className={`p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 ${
                              account.enabled
                                ? "text-gray-400 hover:text-orange-500"
                                : "text-emerald-500 hover:text-emerald-600"
                            }`}
                            title={account.enabled ? "禁用账号" : "启用账号"}
                          >
                            {account.enabled ? (
                              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" />
                              </svg>
                            ) : (
                              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                              </svg>
                            )}
                          </button>
                          <button
                            onClick={() => handleDelete(account.id)}
                            className="p-1.5 rounded text-gray-400 hover:text-red-500 hover:bg-gray-100 dark:hover:bg-gray-700"
                            title="删除账号"
                          >
                            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                            </svg>
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>
      {showProjectPrompt && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4">
          <div className="w-full max-w-md rounded-lg bg-white dark:bg-gray-800 shadow-lg border border-gray-200 dark:border-gray-700 p-6">
            <h4 className="text-lg font-semibold text-gray-800 dark:text-white">
              输入 GCP 项目 ID
            </h4>
            <p className="mt-2 text-sm text-gray-600 dark:text-gray-300">
              Gemini CLI 需要项目 ID 才能请求 Cloud Code Assist。
            </p>
            <input
              type="text"
              value={projectIdInput}
              onChange={(e) => setProjectIdInput(e.target.value)}
              placeholder="例如：my-gcp-project"
              className="mt-4 w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white"
              autoFocus
            />
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={handleProjectCancel}
                className="px-4 py-2 rounded-lg border border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-200 hover:bg-gray-50 dark:hover:bg-gray-700"
              >
                取消
              </button>
              <button
                onClick={handleProjectConfirm}
                className="px-4 py-2 rounded-lg bg-blue-500 hover:bg-blue-600 text-white"
              >
                继续登录
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
