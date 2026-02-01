import { Page, ServerStatus } from "../App";

interface SidebarProps {
  currentPage: Page;
  onPageChange: (page: Page) => void;
  serverStatus: ServerStatus;
}

export function Sidebar({ currentPage, onPageChange, serverStatus }: SidebarProps) {
  const navItems: { id: Page; label: string; icon: string }[] = [
    { id: "dashboard", label: "仪表盘", icon: "📊" },
    { id: "accounts", label: "账户管理", icon: "👤" },
    { id: "settings", label: "设置", icon: "⚙️" },
  ];

  return (
    <aside className="w-64 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col">
      <div className="p-4 border-b border-gray-200 dark:border-gray-700">
        <h1 className="text-xl font-bold text-gray-800 dark:text-white">
          CLI Proxy API
        </h1>
        <div className="mt-2 flex items-center gap-2">
          <span
            className={`w-2 h-2 rounded-full ${
              serverStatus.running ? "bg-green-500" : "bg-red-500"
            }`}
          />
          <span className="text-sm text-gray-600 dark:text-gray-400">
            {serverStatus.running ? "运行中" : "已停止"}
          </span>
          {serverStatus.running && (
            <span className="text-xs text-gray-500 dark:text-gray-500">
              :{serverStatus.port}
            </span>
          )}
        </div>
      </div>

      <nav className="flex-1 p-4">
        <ul className="space-y-2">
          {navItems.map((item) => (
            <li key={item.id}>
              <button
                onClick={() => onPageChange(item.id)}
                className={`w-full flex items-center gap-3 px-4 py-2 rounded-lg transition-colors ${
                  currentPage === item.id
                    ? "bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300"
                    : "text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700"
                }`}
              >
                <span>{item.icon}</span>
                <span>{item.label}</span>
              </button>
            </li>
          ))}
        </ul>
      </nav>

      <div className="p-4 border-t border-gray-200 dark:border-gray-700">
        <p className="text-xs text-gray-500 dark:text-gray-500">
          Tauri Desktop App
        </p>
        <p className="text-xs text-gray-400 dark:text-gray-600">v0.1.0</p>
      </div>
    </aside>
  );
}
