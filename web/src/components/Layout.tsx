import React, { useState } from "react";
import { Outlet, useNavigate, Link } from "react-router";
import { Search, Activity } from "lucide-react";

export function Layout() {
  const navigate = useNavigate();
  const [searchValue, setSearchValue] = useState("");

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    if (!searchValue.trim()) return;

    // Simple heuristic: if it looks like a UUID or short ID, go to flow
    // otherwise search events (for now we might just support flow navigation)
    navigate(`/flow/${searchValue}`);
  };

  return (
    <div className="min-h-screen bg-gradient-to-b from-gray-50 to-gray-100 flex flex-col">
      <header className="bg-white/90 backdrop-blur border-b border-gray-200 sticky top-0 z-10 shadow-sm">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex">
              <Link to="/" className="flex-shrink-0 flex items-center gap-2">
                <Activity className="h-8 w-8 text-indigo-600" />
                <span className="font-bold text-xl text-gray-900">Ojo</span>
              </Link>
              <div className="hidden sm:ml-6 sm:flex sm:space-x-4">
                <Link
                  to="/"
                  className="inline-flex items-center px-3 py-2 text-sm font-medium rounded-md text-gray-900 bg-gray-100 shadow-inner"
                >
                  Dashboard
                </Link>
                <Link
                  to="/flows"
                  className="inline-flex items-center px-3 py-2 text-sm font-medium rounded-md text-gray-600 hover:text-gray-900 hover:bg-gray-100 transition"
                >
                  Flows
                </Link>
              </div>
            </div>
            <div className="flex-1 flex items-center justify-center px-2 lg:ml-6 lg:justify-end">
              <div className="max-w-lg w-full lg:max-w-xs">
                <form onSubmit={handleSearch} className="relative">
                  <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <Search
                      className="h-5 w-5 text-gray-400"
                      aria-hidden="true"
                    />
                  </div>
                  <input
                    type="text"
                    className="block w-full pl-10 pr-4 py-2 border border-gray-200 rounded-full leading-5 bg-white shadow-sm placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 sm:text-sm transition"
                    placeholder="Search flow ID or event..."
                    value={searchValue}
                    onChange={(e) => setSearchValue(e.target.value)}
                  />
                </form>
              </div>
            </div>
          </div>
        </div>
      </header>

      <main className="flex-1 max-w-7xl w-full mx-auto px-4 sm:px-6 lg:px-8 py-10">
        <Outlet />
      </main>
    </div>
  );
}
