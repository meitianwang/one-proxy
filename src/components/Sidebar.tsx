import { Page } from "../App";
import { LayoutDashboard, Users, ScrollText, Settings, ShieldCheck } from "lucide-react";

interface SidebarProps {
  currentPage: Page;
  onPageChange: (page: Page) => void;
}

export function Sidebar({ currentPage, onPageChange }: SidebarProps) {
  const navItems = [
    { id: "dashboard" as Page, label: "仪表盘", icon: LayoutDashboard },
    { id: "accounts" as Page, label: "账号管理", icon: Users },
    { id: "logs" as Page, label: "请求日志", icon: ScrollText },
    { id: "settings" as Page, label: "设置", icon: Settings },
  ];

  return (
    <aside className="w-64 bg-white/80 dark:bg-gray-900/80 backdrop-blur-xl border-r border-gray-200/50 dark:border-gray-800/50 flex flex-col h-full shadow-lg">
      <div className="p-6 flex items-center gap-3">
        <div className="w-10 h-10 rounded-xl bg-gradient-to-tr from-blue-600 to-indigo-500 flex items-center justify-center shadow-md shadow-blue-500/20">
          <ShieldCheck className="w-6 h-6 text-white" />
        </div>
        <h1 className="text-xl font-black bg-clip-text text-transparent bg-gradient-to-r from-gray-900 to-gray-600 dark:from-white dark:to-gray-300 tracking-tight">
          OneProxy
        </h1>
      </div>

      <nav className="flex-1 px-4 py-6 space-y-2">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = currentPage === item.id;
          return (
            <button
              key={item.id}
              onClick={() => onPageChange(item.id)}
              className={`w-full flex items-center gap-3 px-4 py-3 rounded-xl text-sm font-medium transition-all duration-300 ease-in-out group relative overflow-hidden ${
                isActive
                  ? "text-blue-700 dark:text-blue-300 bg-blue-50/80 dark:bg-blue-900/30"
                  : "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white hover:bg-gray-50/80 dark:hover:bg-gray-800/50"
              }`}
            >
              {isActive && (
                <div className="absolute left-0 top-0 bottom-0 w-1 bg-blue-600 dark:bg-blue-500 rounded-r-full" />
              )}
              <Icon className={`w-5 h-5 transition-transform duration-300 group-hover:scale-110 ${isActive ? "text-blue-600 dark:text-blue-400" : ""}`} />
              <span className="relative z-10">{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="p-4 border-t border-gray-200/50 dark:border-gray-800/50 text-center">
        <p className="text-xs text-gray-500 dark:text-gray-500 font-medium">
          OneProxy v0.1.0 ❤️
        </p>
      </div>
    </aside>
  );
}
