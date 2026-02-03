import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface RequestLogEntry {
  id: number;
  status: number;
  method: string;
  model: string | null;
  protocol: string | null;
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
        invoke<RequestLogEntry[]>("get_request_logs", { limit: 100, offset: 0, filter }),
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

  function getProviderFromModel(model: string | null): string {
    if (!model) return "-";
    // Extract provider from model prefix (e.g., "antigravity/gemini-3-pro" -> "Antigravity")
    if (model.includes("/")) {
      const provider = model.split("/")[0].toLowerCase();
      switch (provider) {
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
          return provider.charAt(0).toUpperCase() + provider.slice(1);
      }
    }
    // Infer from model name
    const modelLower = model.toLowerCase();
    if (modelLower.startsWith("gemini")) return "Antigravity";
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
    <div className="space-y-4 h-full flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-gray-100 dark:bg-gray-700 flex items-center justify-center">
            <svg className="w-5 h-5 text-gray-600 dark:text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
            </svg>
          </div>
          <div>
            <h2 className="text-xl font-bold text-gray-800 dark:text-white">请求日志</h2>
            <p className="text-sm text-gray-500 dark:text-gray-400">
              共 {totalCount} 条记录
              {paused && <span className="ml-2 text-yellow-500">(已暂停)</span>}
            </p>
          </div>
        </div>
      </div>

      {/* Filter Tabs */}
      <div className="flex items-center gap-2">
        {TABS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setSelectedTab(tab.id)}
            className={`px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${selectedTab === tab.id
              ? "bg-gray-800 dark:bg-gray-700 text-white"
              : "bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600"
              }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Toolbar */}
      <div className="flex items-center gap-3">
        {/* Pause/Resume */}
        <button
          onClick={() => setPaused(!paused)}
          className={`px-3 py-2 rounded-lg text-sm font-medium flex items-center gap-2 transition-colors ${paused
            ? "bg-green-600 hover:bg-green-700 text-white"
            : "bg-gray-800 hover:bg-gray-900 dark:bg-gray-700 dark:hover:bg-gray-600 text-white"
            }`}
        >
          {paused ? (
            <>
              <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                <path d="M8 5v14l11-7z" />
              </svg>
              继续
            </>
          ) : (
            <>
              <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 19h4V5H6v14zm8-14v14h4V5h-4z" />
              </svg>
              暂停
            </>
          )}
        </button>

        {/* Search */}
        <div className="flex-1 max-w-xs">
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索路径或模型..."
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-white text-sm"
          />
        </div>

        {/* Refresh */}
        <button
          onClick={fetchLogs}
          className="p-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700"
          title="刷新"
        >
          <svg className="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
          </svg>
        </button>

        {/* Clear */}
        <button
          onClick={handleClear}
          className="p-2 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700"
          title="清空日志"
        >
          <svg className="w-5 h-5 text-gray-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
          </svg>
        </button>
      </div>

      {/* Table */}
      <div className="flex-1 bg-white dark:bg-gray-800 rounded-lg shadow overflow-hidden">
        <div className="overflow-auto h-full">
          <table className="w-full text-sm">
            <thead className="bg-gray-50 dark:bg-gray-700 sticky top-0">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">状态</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">方法</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider max-w-48">模型</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-20">协议</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-24">供应商</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider min-w-40">账号</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-16">耗时</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider w-20">时间</th>
              </tr>

            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
              {logs.length === 0 ? (
                <tr>
                  <td colSpan={8} className="px-4 py-8 text-center text-gray-500 dark:text-gray-400">
                    暂无日志记录
                  </td>
                </tr>
              ) : (
                logs.map((log) => (
                  <tr key={log.id} className="hover:bg-gray-50 dark:hover:bg-gray-700">
                    <td className="px-4 py-3 whitespace-nowrap">
                      <div className="flex items-center gap-2">
                        <span className={`w-2 h-2 rounded-full ${getStatusColor(log.status)}`} />
                        <span className="text-gray-800 dark:text-white">{log.status}</span>
                      </div>
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap">
                      <span className="px-2 py-1 text-xs font-medium bg-gray-100 dark:bg-gray-600 text-gray-800 dark:text-white rounded">
                        {log.method}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-gray-600 dark:text-gray-300 max-w-48 truncate" title={log.model || undefined}>
                      {log.model || "-"}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {getProtocolLabel(log.protocol)}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300">
                      {getProviderFromModel(log.model)}
                    </td>
                    <td className="px-4 py-3 whitespace-nowrap text-gray-600 dark:text-gray-300 min-w-40" title={log.account_id || undefined}>
                      {log.account_id ? (log.account_id.length > 24 ? log.account_id.slice(0, 24) + "..." : log.account_id) : "-"}
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
