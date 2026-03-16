import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dashboard } from "./pages/Dashboard";
import { Accounts } from "./pages/Accounts";
import { Settings } from "./pages/Settings";
import { RequestLogs } from "./pages/RequestLogs";
import { Header } from "./components/Header";

export type Page = "dashboard" | "accounts" | "logs" | "settings";

export interface ServerStatus {
  running: boolean;
  port: number;
  host: string;
}

export interface AppConfig {
  host: string;
  port: number;
  debug: boolean;
  "auth-dir": string;
  "api-keys": string[];
  "proxy-url": string;
}

function App() {
  const [currentPage, setCurrentPage] = useState<Page>("dashboard");
  const [serverStatus, setServerStatus] = useState<ServerStatus>({
    running: false,
    port: 8417,
    host: "0.0.0.0",
  });

  useEffect(() => {
    // Fetch initial server status
    fetchServerStatus();

    // Poll server status every 5 seconds
    const interval = setInterval(fetchServerStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  async function fetchServerStatus() {
    try {
      const status = await invoke<ServerStatus>("get_server_status");
      setServerStatus(status);
    } catch (error) {
      console.error("Failed to fetch server status:", error);
    }
  }

  const renderPage = () => {
    switch (currentPage) {
      case "dashboard":
        return (
          <Dashboard
            serverStatus={serverStatus}
            onStatusChange={fetchServerStatus}
          />
        );
      case "accounts":
        return <Accounts />;
      case "logs":
        return <RequestLogs />;
      case "settings":
        return <Settings />;
      default:
        return (
          <Dashboard
            serverStatus={serverStatus}
            onStatusChange={fetchServerStatus}
          />
        );
    }
  };

  return (
    <div className="flex flex-col h-screen bg-[#f6f6f6] dark:bg-[#0a0a0a] text-gray-900 dark:text-gray-100 selection:bg-blue-200 dark:selection:bg-blue-900 overflow-hidden">
      <Header currentPage={currentPage} onPageChange={setCurrentPage} />
      <main className="flex-1 overflow-y-auto overflow-x-hidden p-6 md:p-10">
        <div className="mx-auto max-w-7xl animate-in fade-in slide-in-from-bottom-4 duration-500">
          {renderPage()}
        </div>
      </main>
    </div>
  );
}

export default App;
