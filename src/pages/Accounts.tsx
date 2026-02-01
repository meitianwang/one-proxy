import { useState, useEffect } from "react";
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

const PROVIDERS = [
  { id: "google", name: "Google (Gemini)", icon: "🔵" },
  { id: "openai", name: "OpenAI (Codex)", icon: "🟢" },
  { id: "antigravity", name: "Antigravity", icon: "⚫" },
];

// Map provider names from auth files to display info
const PROVIDER_ALIASES: Record<string, string> = {
  "gemini": "google",
  "codex": "openai",
};

function getProviderIcon(provider: string): string {
  const normalizedProvider = PROVIDER_ALIASES[provider] || provider;
  return PROVIDERS.find((p) => p.id === normalizedProvider)?.icon || "❓";
}

export function Accounts() {
  const [accounts, setAccounts] = useState<AuthAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [loginInProgress, setLoginInProgress] = useState<string | null>(null);

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

  async function handleLogin(provider: string) {
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
        await fetchAccounts();
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

  return (
    <div className="space-y-6">
      <h2 className="text-2xl font-bold text-gray-800 dark:text-white">账户管理</h2>

      {/* Add Account Section */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          添加账户
        </h3>
        <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-4">
          {PROVIDERS.map((provider) => (
            <button
              key={provider.id}
              onClick={() => handleLogin(provider.id)}
              disabled={loginInProgress !== null}
              className={`flex flex-col items-center gap-2 p-4 rounded-lg border border-gray-200 dark:border-gray-700 transition-colors ${
                loginInProgress === provider.id
                  ? "bg-blue-50 dark:bg-blue-900 border-blue-300 dark:border-blue-700"
                  : loginInProgress !== null
                  ? "opacity-50 cursor-not-allowed"
                  : "hover:bg-gray-50 dark:hover:bg-gray-700"
              }`}
            >
              <span className="text-2xl">{provider.icon}</span>
              <span className="text-sm text-gray-700 dark:text-gray-300 text-center">
                {loginInProgress === provider.id ? "登录中..." : provider.name}
              </span>
            </button>
          ))}
        </div>
      </div>

      {/* Existing Accounts */}
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6">
        <h3 className="text-lg font-semibold text-gray-800 dark:text-white mb-4">
          已登录账户
        </h3>

        {loading ? (
          <p className="text-gray-500 dark:text-gray-400">加载中...</p>
        ) : accounts.length === 0 ? (
          <p className="text-gray-500 dark:text-gray-400">
            暂无已登录账户，请点击上方按钮添加账户
          </p>
        ) : (
          <div className="space-y-3">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center justify-between p-4 rounded-lg border border-gray-200 dark:border-gray-700"
              >
                <div className="flex items-center gap-4">
                  <span className="text-2xl">
                    {getProviderIcon(account.provider)}
                  </span>
                  <div>
                    <p className="font-medium text-gray-800 dark:text-white">
                      {account.email || account.id}
                    </p>
                    <p className="text-sm text-gray-500 dark:text-gray-400">
                      {account.provider}
                      {account.prefix && ` (前缀: ${account.prefix})`}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <span
                    className={`px-2 py-1 text-xs rounded ${
                      account.enabled
                        ? "bg-green-100 text-green-700 dark:bg-green-900 dark:text-green-300"
                        : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300"
                    }`}
                  >
                    {account.enabled ? "已启用" : "已禁用"}
                  </span>
                  <button
                    onClick={() => handleDelete(account.id)}
                    className="px-2 py-1 text-xs rounded bg-red-100 text-red-700 dark:bg-red-900 dark:text-red-300 hover:bg-red-200 dark:hover:bg-red-800 transition-colors"
                  >
                    删除
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
