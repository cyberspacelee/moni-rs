import type {
  AlertEvent,
  AlertRule,
  AlertRuleInput,
  AlertRulePatch,
  AlertTestResponse,
  ActiveAlert,
  AppConfig,
  CreateServerResponse,
  History,
  HistoryRange,
  ServerStatus,
  TokenResponse,
} from "../api/generated";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** Fired when any request comes back 401 so the app can drop back to the login screen. */
export const UNAUTHORIZED_EVENT = "moni:unauthorized";

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let code = "internal";
    let message = res.statusText;
    try {
      const err = (await res.json()) as { error?: string; message?: string };
      code = err.error ?? code;
      message = err.message ?? message;
    } catch {
      // non-JSON error body
    }
    if (res.status === 401 && path !== "/api/auth/login") window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    throw new ApiError(res.status, code, message);
  }
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  return (text ? JSON.parse(text) : undefined) as T;
}

export const api = {
  login: (password: string) => request<void>("POST", "/api/auth/login", { password }),
  logout: () => request<void>("POST", "/api/auth/logout"),
  me: () => request<{ authenticated: boolean }>("GET", "/api/auth/me"),
  config: () => request<AppConfig>("GET", "/api/config"),

  servers: () => request<ServerStatus[]>("GET", "/api/servers"),
  createServer: (name: string, id?: string) =>
    request<CreateServerResponse>("POST", "/api/servers", id ? { name, id } : { name }),
  renameServer: (id: string, name: string) =>
    request<ServerStatus>("PATCH", `/api/servers/${encodeURIComponent(id)}`, { name }),
  deleteServer: (id: string) => request<void>("DELETE", `/api/servers/${encodeURIComponent(id)}`),
  rotateToken: (id: string) => request<TokenResponse>("POST", `/api/servers/${encodeURIComponent(id)}/token`),
  history: (id: string, range: HistoryRange) =>
    request<History>("GET", `/api/servers/${encodeURIComponent(id)}/history?range=${range}`),

  rules: () => request<AlertRule[]>("GET", "/api/alerts/rules"),
  createRule: (input: AlertRuleInput) => request<AlertRule>("POST", "/api/alerts/rules", input),
  updateRule: (id: string, patch: AlertRulePatch) =>
    request<AlertRule>("PATCH", `/api/alerts/rules/${encodeURIComponent(id)}`, patch),
  deleteRule: (id: string) => request<void>("DELETE", `/api/alerts/rules/${encodeURIComponent(id)}`),
  activeAlerts: () => request<ActiveAlert[]>("GET", "/api/alerts/active"),
  events: (limit = 50, beforeId?: number) =>
    request<AlertEvent[]>(
      "GET",
      `/api/alerts/events?limit=${limit}${beforeId == null ? "" : `&before_id=${beforeId}`}`,
    ),
  testNotify: () => request<AlertTestResponse>("POST", "/api/alerts/test"),
};

export function errorText(e: unknown): string {
  if (e instanceof ApiError) return e.message || e.code;
  if (e instanceof Error) return e.message;
  return String(e);
}
