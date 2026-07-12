export const SYSTEM_TIMEZONE = "system";

const FALLBACK_TIMEZONES = [
  "UTC",
  "Europe/London",
  "Europe/Paris",
  "Europe/Berlin",
  "Europe/Madrid",
  "Europe/Rome",
  "Europe/Amsterdam",
  "Europe/Brussels",
  "Europe/Zurich",
  "Europe/Stockholm",
  "Europe/Warsaw",
  "Europe/Athens",
  "Europe/Istanbul",
  "Africa/Casablanca",
  "Africa/Johannesburg",
  "America/New_York",
  "America/Chicago",
  "America/Denver",
  "America/Los_Angeles",
  "America/Toronto",
  "America/Sao_Paulo",
  "America/Mexico_City",
  "Asia/Dubai",
  "Asia/Kolkata",
  "Asia/Singapore",
  "Asia/Tokyo",
  "Asia/Seoul",
  "Asia/Shanghai",
  "Australia/Sydney",
  "Pacific/Auckland",
];

export function getSystemTimezone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

export function getSupportedTimezones(): string[] {
  try {
    if (typeof Intl !== "undefined" && "supportedValuesOf" in Intl) {
      return [...Intl.supportedValuesOf("timeZone")].sort();
    }
  } catch {
    // Fall through to static list.
  }
  return [...FALLBACK_TIMEZONES];
}

export function normalizeTimezoneSetting(value: string | undefined | null): string {
  const trimmed = value?.trim();
  if (!trimmed || trimmed === SYSTEM_TIMEZONE) {
    return SYSTEM_TIMEZONE;
  }
  return trimmed;
}

export function resolveTimeZone(setting: string): string | undefined {
  const normalized = normalizeTimezoneSetting(setting);
  if (normalized === SYSTEM_TIMEZONE) {
    return undefined;
  }
  return normalized;
}

function formatWithTimezone(
  iso: string,
  setting: string,
  mode: "datetime" | "date",
): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return iso;
  }

  const timeZone = resolveTimeZone(setting);
  const options: Intl.DateTimeFormatOptions =
    mode === "datetime"
      ? {
          year: "numeric",
          month: "short",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        }
      : {
          year: "numeric",
          month: "short",
          day: "numeric",
        };

  if (timeZone) {
    options.timeZone = timeZone;
  }

  return mode === "datetime"
    ? date.toLocaleString(undefined, options)
    : date.toLocaleDateString(undefined, options);
}

export function formatDateTime(iso: string, setting: string): string {
  return formatWithTimezone(iso, setting, "datetime");
}

export function formatDate(iso: string, setting: string): string {
  return formatWithTimezone(iso, setting, "date");
}

export function formatTimezoneLabel(setting: string): string {
  if (normalizeTimezoneSetting(setting) === SYSTEM_TIMEZONE) {
    return `System (${getSystemTimezone()})`;
  }
  return setting;
}
