import type { Connection } from "$lib/types";

/** Mirrors Rust `Connection::to_ssh_command`. */
export function toSshCommand(conn: Connection): string {
  let cmd = conn.use_kerberos ? "ssh -t -A -K " : "ssh ";

  if (conn.key_path) {
    cmd += `-i ${conn.key_path} `;
  }

  if (conn.bastion) {
    const bastionUser = conn.bastion_user || conn.user;
    cmd += `-p 22 ${bastionUser}@${conn.bastion}`;
    cmd += ` ${conn.user}@${conn.host}`;
  } else {
    cmd += `-p ${conn.port} ${conn.user}@${conn.host}`;
  }

  return cmd;
}
