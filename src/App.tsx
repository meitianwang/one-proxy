import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Dashboard } from "./pages/Dashboard";
import { Accounts } from "./pages/Accounts";
import { Sidebar } from "./components/Sidebar";

export type Page = "dashboard" | "accounts";

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
        return <Dashboard serverStatus={serverStatus} onStatusChange={fetchServerStatus} />;
      case "accounts":
        return <Accounts />;
      default:
        return <Dashboard serverStatus={serverStatus} onStatusChange={fetchServerStatus} />;
    }
  };

  return (
    <div className="flex h-screen bg-gray-100 dark:bg-gray-900">
      <Sidebar
        currentPage={currentPage}
        onPageChange={setCurrentPage}
        serverStatus={serverStatus}
      />
      <main className="flex-1 overflow-auto p-6">
        {renderPage()}
      </main>
    </div>
  );
}

export default App;
