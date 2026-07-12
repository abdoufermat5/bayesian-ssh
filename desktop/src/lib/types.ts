export interface Connection {
  id: string;
  name: string;
  host: string;
  user: string;
  port: number;
  bastion?: string;
  bastion_user?: string;
  use_kerberos: boolean;
  key_path?: string;
  created_at: string;
  last_used?: string;
  tags: string[];
}

export interface EnvInfo {
  name: string;
  is_active: boolean;
}

export interface SessionHistoryEntry {
  id: string;
  name: string;
  started_at: string;
  ended_at?: string;
  status: string;
  exit_code?: number;
}

export interface ConnectionStats {
  total_connections: number;
  most_used?: Connection;
  recently_used: Connection[];
  by_tag: Record<string, number>;
}

export interface DesktopSettings {
  theme: string;
  auto_start_agent: boolean;
  custom_agent_socket: string;
  default_user: string;
  default_port: number;
  fuzzy_search: boolean;
  default_key_path: string;
}

export type AppTab = "connections" | "terminals" | "history" | "settings";
export type NotificationType = "success" | "error" | "info";
