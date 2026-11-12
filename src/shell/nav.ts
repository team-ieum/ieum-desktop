import { Globe, History, Laptop, LayoutDashboard, Settings, type LucideIcon } from "lucide-react";
import type { Key } from "../i18n";

export type Page = "dashboard" | "local" | "web" | "history" | "settings";

export type View =
  | { page: Page }
  | { page: "local"; editor: string | null }
  | { page: "local"; run: string }
  | { page: "web"; workflow: string };

export const NAV: { page: Page; label: Key; icon: LucideIcon }[] = [
  { page: "dashboard", label: "nav.dashboard", icon: LayoutDashboard },
  { page: "local", label: "nav.local", icon: Laptop },
  { page: "web", label: "nav.web", icon: Globe },
  { page: "history", label: "nav.history", icon: History },
  { page: "settings", label: "nav.settings", icon: Settings },
];
