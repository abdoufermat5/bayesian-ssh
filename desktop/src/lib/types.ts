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
  connection_name: string;
  started_at: string;
  ended_at?: string;
  status: string | { Error: string };
  exit_code?: number;
  duration?: number;
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
  timezone: string;
  onboarding_complete?: boolean;
}

export interface WorkspaceInfo {
  active_env: string;
  config_root: string;
  env_dir: string;
  config_path: string;
  database_path: string;
  ssh_config_path?: string | null;
  default_user: string;
  default_port: number;
  search_mode: string;
  log_level: string;
  auto_save_history: boolean;
  max_history_size: number;
}

export interface OnboardingPayload {
  profile_name: string;
  create_profile: boolean;
  default_user: string;
  default_port: number;
  ssh_config_path?: string | null;
  theme: string;
  auto_start_agent: boolean;
  import_ssh_config: boolean;
  fuzzy_search: boolean;
}

export type AppTab = "connections" | "terminals" | "history" | "settings";
export type NotificationType = "success" | "error" | "info";
