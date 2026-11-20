/** Why a connection attempt ended, in plain words, plus ssh's own last lines. */
export interface ConnectFailure {
  title: string;
  hint: string;
  detail: string;
}

/** Prefix spawn_pty uses for a team-sync route change waiting for confirmation. */
const PENDING_CHANGE_PREFIX = "connection-change-pending:";

/** A team-sync route change the desktop must show before connecting. */
export interface PendingChangeError {
  name: string;
  detected_at: string;
  changes: string[];
}

export function parsePendingChangeError(e: unknown): PendingChangeError | null {
  const message = String(e);
  if (!message.startsWith(PENDING_CHANGE_PREFIX)) return null;
  try {
    return JSON.parse(message.slice(PENDING_CHANGE_PREFIX.length)) as PendingChangeError;
  } catch {
    return null;
  }
}

const ANSI_PATTERN = new RegExp(
  String.raw`\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]`,
  "g",
);

export function stripTerminalControls(data: string): string {
  return data.replace(ANSI_PATTERN, "").replace(/\r/g, "");
}

/** Plain-words explanation of the last ssh output; the raw lines stay as detail. */
export function describeConnectFailure(output: string): ConnectFailure {
  const detail = output
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(-6)
    .join("\n");
  const has = (re: RegExp) => re.test(detail);
  if (has(/REMOTE HOST IDENTIFICATION HAS CHANGED/i)) {
    return {
      title: "The host's identity changed",
      hint: "Its host key no longer matches the one saved earlier. Don't connect until you know why: it can mean a reinstall, or someone in the middle.",
      detail,
    };
  }
  if (has(/host key verification failed/i)) {
    return {
      title: "The host key wasn't accepted",
      hint: "ssh couldn't verify this host's key. Check it with the host's owner, then add it to known_hosts.",
      detail,
    };
  }
  if (has(/permission denied|too many authentication failures|no supported authentication/i)) {
    return {
      title: "The host rejected your credentials",
      hint: "Check the user name and the key or password. Edit the host to change them.",
      detail,
    };
  }
  if (has(/could not resolve hostname|name or service not known|nodename nor servname/i)) {
    return {
      title: "That host name can't be found",
      hint: "Check the address for typos, and that your network or VPN is up.",
      detail,
    };
  }
  if (has(/connection refused/i)) {
    return {
      title: "The host refused the connection",
      hint: "Nothing accepted the connection on that port. Check the port, and that the SSH service is running.",
      detail,
    };
  }
  if (has(/timed out|no route to host|network is unreachable/i)) {
    return {
      title: "The host didn't answer",
      hint: "It may be offline, firewalled, or only reachable through a VPN or bastion.",
      detail,
    };
  }
  if (has(/connection closed|connection reset|broken pipe|kex_exchange_identification/i)) {
    return {
      title: "The host closed the connection",
      hint: "It hung up before login finished. A firewall, a connection limit or a bastion setting may be the cause.",
      detail,
    };
  }
  return {
    title: "ssh couldn't connect",
    hint: detail ? "ssh reported:" : "ssh stopped without saying why.",
    detail,
  };
}
