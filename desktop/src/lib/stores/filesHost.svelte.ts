/** A one-shot request for the Files view to open on a given host. */

let pending = $state<string | null>(null);

/** Ask the Files view to select this connection; the caller switches to the sftp tab. */
export function requestFilesHost(connectionId: string) {
  pending = connectionId;
}

/** Returns and clears the pending request. */
export function takeFilesHost(): string | null {
  const id = pending;
  pending = null;
  return id;
}

export function pendingFilesHost(): string | null {
  return pending;
}
