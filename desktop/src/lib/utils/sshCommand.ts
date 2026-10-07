import type { Connection } from "$lib/types";

/** Mirrors Rust `Connection::to_ssh_command`. */
export function toSshCommand(conn: Connection): string {
  let cmd = conn.use_kerberos ? "ssh -t -A -K " : "ssh ";

  if (conn.key_path) {
    // Validation rejects `'` in key paths, so single quotes are safe.
    const key = /^[A-Za-z0-9/._~-]+$/.test(conn.key_path) ? conn.key_path : `'${conn.key_path}'`;
    cmd += `-i ${key} `;
  }

  if (conn.bastion) {
    const bastionUser = conn.bastion_user || conn.user;
    if (conn.use_kerberos) {
      // Interactive bastion: log into the bastion and hand it the target.
      cmd += `-p 22 ${bastionUser}@${conn.bastion} ${conn.user}@${conn.host}`;
    } else {
      cmd += `-J ${bastionUser}@${conn.bastion} -p ${conn.port} ${conn.user}@${conn.host}`;
    }
  } else {
    cmd += `-p ${conn.port} ${conn.user}@${conn.host}`;
  }

  return cmd;
}
