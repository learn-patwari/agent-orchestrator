import "./globals.css";
import type { Viewport } from "next";
import type { ReactNode } from "react";
import Sidebar from "../components/Sidebar";
import TokenBar from "../components/TokenBar";

export const metadata = { title: "Agent Orchestrator", description: "A team of AI agents for software development" };
export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f6f7fb" },
    { media: "(prefers-color-scheme: dark)", color: "#0d0f14" },
  ],
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>
        <a className="skip-link" href="#main">
          Skip to content
        </a>
        <div className="shell">
          <Sidebar />
          <div className="content">
            <TokenBar />
            <main id="main" tabIndex={-1}>
              {children}
            </main>
          </div>
        </div>
      </body>
    </html>
  );
}
