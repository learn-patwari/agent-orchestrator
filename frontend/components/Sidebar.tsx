"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { ReactNode } from "react";

const icon = (d: ReactNode) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {d}
  </svg>
);

const GROUPS: { label: string; items: { href: string; name: string; icon: ReactNode }[] }[] = [
  {
    label: "Work",
    items: [
      { href: "/", name: "Home", icon: icon(<path d="M3 11l9-8 9 8v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z" />) },
      { href: "/chat", name: "Chat", icon: icon(<path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" />) },
      { href: "/features", name: "Features", icon: icon(<path d="M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5M3 17.5l9 5 9-5" />) },
      { href: "/pipeline", name: "Pipeline", icon: icon(<path d="M6 3v12m0 0a3 3 0 1 0 0 6 3 3 0 0 0 0-6zm12-6a3 3 0 1 0 0-6 3 3 0 0 0 0 6zm0 0v3a4 4 0 0 1-4 4h-3" />) },
      { href: "/inbox", name: "Inbox", icon: icon(<path d="M3 13l3-8h12l3 8v6a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-6zm0 0h5l1 3h6l1-3h5" />) },
    ],
  },
  {
    label: "Team",
    items: [{ href: "/registry", name: "External agents", icon: icon(<path d="M12 2v4m0 12v4M2 12h4m12 0h4M7 7l2 2m6 6l2 2m0-10l-2 2M9 15l-2 2M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z" />) }, { href: "/status", name: "Agents", icon: icon(<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8zm13 10v-2a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8" />) }],
  },
  {
    label: "Setup",
    items: [
      { href: "/providers", name: "Providers", icon: icon(<path d="M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0V7zm6 10v5m-4 3h8" />) },
      { href: "/profiles", name: "Token profiles", icon: icon(<path d="M12 3v18M5 8h14M7 13h10M9 18h6" />) },
      { href: "/settings", name: "Settings", icon: icon(<path d="M4 6h10m4 0h2M4 12h4m4 0h8M4 18h12m4 0h0M16 4v4M10 10v4M18 16v4" />) },
    ],
  },
];

export default function Sidebar() {
  const path = usePathname() || "/";
  return (
    <aside className="sidebar">
      <div className="brand">
        <div className="brand-mark" aria-hidden="true">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="3" />
            <path d="M12 2v4M12 18v4M2 12h4M18 12h4M5 5l3 3M16 16l3 3M19 5l-3 3M8 16l-3 3" />
          </svg>
        </div>
        <span>Orchestrator</span>
      </div>
      <nav className="nav" aria-label="Main">
        {GROUPS.map((g) => (
          <div key={g.label} style={{ display: "contents" }}>
            <div className="nav-label">{g.label}</div>
            {g.items.map((i) => (
              <Link key={i.href} href={i.href} aria-current={(i.href === "/" ? path === "/" : path.startsWith(i.href)) ? "page" : undefined}>
                {i.icon}
                {i.name}
              </Link>
            ))}
          </div>
        ))}
      </nav>
      <div className="sidebar-foot">Agent Orchestrator v2</div>
    </aside>
  );
}
