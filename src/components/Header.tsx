import { Page } from "../App";

interface HeaderProps {
  currentPage: Page;
  onPageChange: (page: Page) => void;
}

export function Header({ currentPage, onPageChange }: HeaderProps) {
  const navItems: { id: Page; label: string }[] = [
    { id: "dashboard", label: "仪表盘" },
    { id: "accounts", label: "账号管理" },
    { id: "settings", label: "设置" },
  ];

  return (
    <header className="bg-white dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700 px-6 py-3">
      <div className="flex items-center relative">
        {/* Logo and App Name */}
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-bold text-gray-800 dark:text-white">
            OneProxy
          </h1>
        </div>

        {/* Navigation Tabs - Centered */}
        <nav className="absolute left-1/2 -translate-x-1/2">
          <div className="flex items-center bg-gray-100 dark:bg-gray-700 rounded-full p-1">
            {navItems.map((item) => (
              <button
                key={item.id}
                onClick={() => onPageChange(item.id)}
                className={`px-5 py-2 rounded-full text-sm font-medium transition-all ${
                  currentPage === item.id
                    ? "bg-gray-800 dark:bg-gray-900 text-white shadow-sm"
                    : "text-gray-600 dark:text-gray-300 hover:text-gray-800 dark:hover:text-white"
                }`}
              >
                {item.label}
              </button>
            ))}
          </div>
        </nav>
      </div>
    </header>
  );
}
