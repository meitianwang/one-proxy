import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ScrollText,
  Pause,
  Play,
  Search,
  RefreshCw,
  Trash2,
} from "lucide-react";

interface RequestLogEntry {
  id: number;
  status: number;
  method: string;
  model: string | null;
  protocol: string | null;
  provider: string | null;
  account_id: string | null;
  path: string;
  input_tokens: number;
  output_tokens: number;
  duration_ms: number;
  timestamp: number;
  error_message: string | null;
}

interface LogFilter {
  errors_only: boolean;
  protocol: string | null;
  search: string | null;
  account_id: string | null;
}

type TabType = "all" | "errors" | "openai" | "gemini" | "anthropic";

const TABS: { id: TabType; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "errors", label: "仅错误" },
  { id: "openai", label: "Chat" },
  { id: "gemini", label: "Gemini" },
  { id: "anthropic", label: "Claude" },
];

export function RequestLogs() {
  const [logs, setLogs] = useState<RequestLogEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [paused, setPaused] = useState(false);
  const [search, setSearch] = useState("");
  const [selectedTab, setSelectedTab] = useState<TabType>("all");
  const [totalCount, setTotalCount] = useState(0);

  const buildFilter = useCallback((): LogFilter => {
    const filter: LogFilter = {
      errors_only: selectedTab === "errors",
      protocol: null,
      search: search.trim() || null,
      account_id: null,
    };

    if (selectedTab === "openai") {
      filter.protocol = "openai";
    } else if (selectedTab === "gemini") {
      filter.protocol = "gemini";
    } else if (selectedTab === "anthropic") {
      filter.protocol = "anthropic";
    }

    return filter;
  }, [selectedTab, search]);

  const fetchLogs = useCallback(async () => {
    try {
      const filter = buildFilter();
      const [logsResult, countResult] = await Promise.all([
        invoke<RequestLogEntry[]>("get_request_logs", {
          limit: 100,
          offset: 0,
          filter,
        }),
        invoke<number>("get_request_logs_count", { filter }),
      ]);
      setLogs(logsResult);
      setTotalCount(countResult);
    } catch (error) {
      console.error("Failed to fetch logs:", error);
    } finally {
      setLoading(false);
    }
  }, [buildFilter]);

  useEffect(() => {
    fetchLogs();
  }, [fetchLogs]);

  useEffect(() => {
    if (paused) return;
    const interval = setInterval(fetchLogs, 2000);
    return () => clearInterval(interval);
  }, [paused, fetchLogs]);

  async function handleClear() {
    try {
      await invoke("clear_request_logs");
      setLogs([]);
      setTotalCount(0);
    } catch (error) {
      console.error("Failed to clear logs:", error);
    }
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts);
    return date.toLocaleTimeString("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function formatDuration(ms: number): string {
    if (ms < 1000) {
      return `${ms}ms`;
    }
    return `${(ms / 1000).toFixed(2)}s`;
  }

  function getStatusColor(status: number): string {
    if (status >= 200 && status < 300) {
      return "bg-green-500";
    } else if (status >= 400) {
      return "bg-red-500";
    }
    return "bg-yellow-500";
  }

  function getProtocolLabel(protocol: string | null): string {
    switch (protocol) {
      case "openai":
        return "OpenAI";
      case "anthropic":
        return "Anthropic";
      case "gemini":
        return "Gemini";
      default:
        return "-";
    }
  }

  function getProviderDisplay(
    provider: string | null,
    model: string | null,
  ): string {
    // Use backend-provided provider if available
    if (provider) {
      // Capitalize first letter
      return provider.charAt(0).toUpperCase() + provider.slice(1);
    }

    // Fallback: infer from model name if no provider from backend
    if (!model) return "-";

    // Check if model has explicit provider prefix (e.g., "antigravity/gemini-3-pro")
    if (model.includes("/")) {
      const prov = model.split("/")[0].toLowerCase();
      switch (prov) {
        case "antigravity":
          return "Antigravity";
        case "codex":
          return "Codex";
        case "kiro":
          return "Kiro";
        case "gemini":
          return "Gemini";
        case "claude":
          return "Claude";
        case "deepseek":
          return "DeepSeek";
        case "kimi":
          return "Kimi";
        case "glm":
        case "zhipu":
          return "GLM";
        default:
          return prov.charAt(0).toUpperCase() + prov.slice(1);
      }
    }

    // Fallback: infer from model name
    const modelLower = model.toLowerCase();
    if (modelLower.startsWith("gemini")) return "Antigravity";
    // Claude thinking models are only supported by Antigravity
    if (modelLower.includes("claude") && modelLower.includes("-thinking"))
      return "Antigravity";
    if (modelLower.startsWith("claude")) return "Kiro";
    if (modelLower.startsWith("gpt")) return "Codex";
    if (modelLower.includes("deepseek")) return "DeepSeek";
    if (modelLower.includes("glm")) return "GLM";
    if (modelLower.includes("kimi")) return "Kimi";
    return "-";
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <span className="text-gray-500 dark:text-gray-400">加载中...</span>
      </div>
    );
  }

  return (
    <div className="max-w-[1400px] mx-auto space-y-6 animate-in mt-4 h-full flex flex-col">
      {/* Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="w-14 h-14 rounded-2xl bg-gradient-to-br from-emerald-500 to-teal-600 flex items-center justify-center shadow-lg shadow-emerald-500/20">
            <ScrollText className="w-7 h-7 text-white" />
          </div>
          <div>
            <h2 className="text-3xl font-black tracking-tight text-gray-900 dark:text-white">
              请求日志
            </h2>
            <div className="text-sm font-medium flex items-center gap-2 mt-1">
              <span className="text-gray-500 dark:text-gray-400">
                共{" "}
                <span className="text-gray-900 dark:text-white mx-0.5">
                  {totalCount}
                </span>{" "}
                条记录
              </span>
              {paused && (
                <span className="px-1.5 py-0.5 rounded-md bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-500 text-[10px] uppercase font-bold tracking-wider animate-pulse">
                  已暂停
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-4 p-4 bg-white/60 dark:bg-gray-900/40 backdrop-blur-md rounded-2xl border border-gray-200/50 dark:border-gray-800/50 shadow-sm">
          {/* Filter Tabs */}
          <div className="flex items-center gap-1.5 bg-gray-100/80 dark:bg-gray-800/80 p-1.5 rounded-xl border border-gray-200/50 dark:border-gray-700/50 shadow-inner overflow-x-auto w-full md:w-auto">
            {TABS.map((tab) => (
              <button
                key={tab.id}
                onClick={() => setSelectedTab(tab.id)}
                className={`px-3 py-1.5 rounded-lg text-sm font-bold transition-all duration-300 whitespace-nowrap ${
                  selectedTab === tab.id
                    ? "bg-white dark:bg-gray-700 text-emerald-600 dark:text-emerald-400 shadow-[0_2px_8px_-2px_rgba(0,0,0,0.1)] scale-100"
                    : "text-gray-500 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 hover:bg-white/50 dark:hover:bg-gray-700/50 scale-95"
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {/* Toolbar */}
          <div className="flex flex-wrap items-center gap-3 w-full md:w-auto">
            {/* Pause/Resume */}
            <button
              onClick={() => setPaused(!paused)}
              className={`px-4 py-2 rounded-xl text-sm font-bold flex items-center justify-center gap-2 transition-all shadow-sm active:scale-95 flex-1 md:flex-none ${
                paused
                  ? "bg-emerald-100/80 text-emerald-700 hover:bg-emerald-200 dark:bg-emerald-900/30 dark:text-emerald-400 dark:hover:bg-emerald-900/50 border border-emerald-200/50 dark:border-emerald-800/50"
                  : "bg-gray-900 hover:bg-gray-800 dark:bg-white dark:hover:bg-gray-100 text-white dark:text-gray-900"
              }`}
            >
              {paused ? (
                <>
                  <Play className="w-4 h-4 fill-current" />
                  继续
                </>
              ) : (
                <>
                  <Pause className="w-4 h-4 fill-current" />
                  暂停
                </>
              )}
            </button>

            <div className="h-8 w-px bg-gray-200 dark:bg-gray-800 hidden md:block mx-1" />

            {/* Search */}
            <div className="relative flex-1 md:w-64">
              <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                <Search className="w-4 h-4 text-gray-400" />
              </div>
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="搜索路径或模型..."
                className="w-full pl-9 pr-4 py-2 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-xl text-sm text-gray-900 dark:text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-emerald-500/20 focus:border-emerald-500 transition-all"
              />
            </div>

            <div className="h-8 w-px bg-gray-200 dark:bg-gray-800 hidden md:block mx-1" />

            {/* Action Buttons */}
            <div className="flex items-center gap-1.5 p-1 bg-white/50 dark:bg-gray-900/50 backdrop-blur-sm rounded-xl border border-gray-200/50 dark:border-gray-800/50 shadow-sm">
              <button
                onClick={fetchLogs}
                className="p-2 rounded-lg text-gray-600 hover:text-gray-900 hover:bg-gray-100/80 dark:text-gray-400 dark:hover:text-white dark:hover:bg-gray-800/80 transition-all font-medium"
                title="刷新"
              >
                <RefreshCw className="w-4 h-4" />
              </button>

              <button
                onClick={handleClear}
                className="p-2 rounded-lg text-red-500 hover:text-red-700 hover:bg-red-50/80 dark:text-red-400 dark:hover:text-red-300 dark:hover:bg-red-900/30 transition-all font-medium"
                title="清空日志"
              >
                <Trash2 className="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Table */}
      <div className="flex-1 bg-white dark:bg-gray-800 rounded-lg shadow overflow-hidden">
        <div className="overflow-auto h-full">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 dark:bg-gray-700 sticky top-0">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">
                  状态
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">
                  方法
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider max-w-48">
                  模型
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-20">
                  协议
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-24">
                  供应商
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider min-w-40">
                  账号
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">
                  耗时
                </th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-20">
                  时间
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
              {logs.length === 0 ? (
                <tr>
                  <td
                    colSpan={8}
                    className="px-4 py-8 text-center text-gray-500 dark:text-gray-400"
                  >
                    暂无日志记录
                  </td>
                </tr>
              ) : (
                logs.map((log) => (
                  <tr
                    key={log.id}
                    className="hover:bg-gray-50 dark:hover:bg-gray-700"
                  >
                    <td className="px-4 py-3 whitespace-nowrap">
                      <div className="flex items-center gap-2">
                        <span
                          className={`w-2 h-2 rounded-full ${getStatusColor(log.status)}`}
                        />
                        <span className="text-gray-800 dark:text-white">
                          {log.status}
                        </span>
                      </div>
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap">
                      <span className="px-2 py-1 text-xs font-medium bg-gray-100 dark:bg-gray-600 text-gray-800 dark:text-white rounded">
                        {log.method}
                      </span>
                    </td>
                    <td
                      className="px-4 py-3 text-gray-600 dark:text-gray-300 max-w-48 truncate"
                      title={log.model || undefined}
                    >
                      {log.model || "-"}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {getProtocolLabel(log.protocol)}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {getProviderDisplay(log.provider, log.model)}
                    </td>
                    <td
                      className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300 min-w-40"
                      title={log.account_id || undefined}
                    >
                      {log.account_id
                        ? log.account_id.length > 24
                          ? log.account_id.slice(0, 24) + "..."
                          : log.account_id
                        : "-"}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {formatDuration(log.duration_ms)}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {formatTimestamp(log.timestamp)}
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
