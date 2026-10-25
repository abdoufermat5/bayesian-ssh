import type { Connection } from "$lib/types";

/**
 * Split a stored bastion (`host`, `host:port`, `[v6]` or `[v6]:port`) the way
 * Rust `split_bastion` does: the destination ssh accepts, plus an explicit port
 * only when one is stored.
 */
function splitBastion(bastion: string): { host: string; port?: number } {
  const validPort = (value: string): number | undefined => {
    const port = Number(value);
    return Number.isInteger(port) && port > 0 && port <= 65535 ? port : undefined;
  };

  if (bastion.startsWith("[")) {
    const end = bastion.indexOf("]");
    if (end === -1) return { host: bastion };
    const host = bastion.slice(0, end + 1);
    const rest = bastion.slice(end + 1);
    return { host, port: rest.startsWith(":") ? validPort(rest.slice(1)) : undefined };
  }
  // An unbracketed IPv6 address never carries a port here.
  if (bastion.split(":").length > 2) return { host: bastion };
  const colon = bastion.indexOf(":");
  if (colon === -1) return { host: bastion };
  const port = validPort(bastion.slice(colon + 1));
  return port === undefined ? { host: bastion } : { host: bastion.slice(0, colon), port };
}

/** Mirrors Rust `Connection::to_ssh_command`. */
export function toSshCommand(conn: Connection): string {
  let cmd = conn.use_kerberos ? "ssh -t " : "ssh ";
  // Agent forwarding is a per-connection opt-in; the Kerberos TGT (-K) is separate.
  if (conn.forward_agent) cmd += "-A ";
  if (conn.use_kerberos) cmd += "-K ";

  if (conn.key_path) {
    // Validation rejects `'` in key paths, so single quotes are safe.
    const key = /^[A-Za-z0-9/._~-]+$/.test(conn.key_path) ? conn.key_path : `'${conn.key_path}'`;
    cmd += `-i ${key} `;
  }

  if (conn.bastion) {
    const bastionUser = conn.bastion_user || conn.user;
    const bastion = splitBastion(conn.bastion);
    if (conn.use_kerberos) {
      // Interactive bastion: log into the bastion and hand it the target.
      // Its port is only pinned when the stored value has one.
      if (bastion.port) cmd += `-p ${bastion.port} `;
      cmd += `${bastionUser}@${bastion.host} ${conn.user}@${conn.host}`;
    } else {
      cmd += `-J ${bastionUser}@${conn.bastion} -p ${conn.port} ${conn.user}@${conn.host}`;
    }
  } else {
    cmd += `-p ${conn.port} ${conn.user}@${conn.host}`;
  }

  return cmd;
}
