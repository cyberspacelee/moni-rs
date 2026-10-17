import { useCallback, useEffect, useState } from "react";
import type { AppConfig } from "./api/generated";
import { Layout } from "./components/Layout";
import { Spinner } from "./components/ui";
import { api, UNAUTHORIZED_EVENT } from "./lib/api";
import { connectLive, disconnectLive, nudgeLive } from "./lib/live";
import { navigate, useRoute } from "./lib/router";
import { Login } from "./pages/Login";
import { Overview } from "./pages/Overview";
import { ServerDetail } from "./pages/ServerDetail";
import { Settings } from "./pages/Settings";

function RedirectHome() {
  useEffect(() => navigate({ page: "overview" }), []);
  return null;
}

/** admin: signed in. public: anonymous read-only view. locked: the deployment requires login. */
type Role = "checking" | "admin" | "public" | "locked";

export function App() {
  const [role, setRole] = useState<Role>("checking");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const route = useRoute();

  const signedIn = useCallback(() => {
    setRole("admin");
    api.config().then(setConfig, () => setConfig(null));
  }, []);

  const signedOut = useCallback(() => {
    setConfig(null);
    api.publicInfo().then(
      (p) => setRole(p.enabled ? "public" : "locked"),
      () => setRole("locked"),
    );
  }, []);

  useEffect(() => {
    api.me().then(signedIn, signedOut);
    window.addEventListener(UNAUTHORIZED_EVENT, signedOut);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, signedOut);
  }, [signedIn, signedOut]);

  // The server picks the dashboard audience at handshake, so a role change needs a new socket.
  const live = role === "admin" || role === "public";
  useEffect(() => {
    if (!live) return;
    connectLive();
    const onVisible = () => document.visibilityState === "visible" && nudgeLive();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      disconnectLive();
    };
  }, [live, role]);

  if (role === "checking")
    return (
      <div className="grid min-h-dvh place-items-center">
        <Spinner className="size-5" />
      </div>
    );

  const admin = role === "admin";
  const needsLogin = role === "locked" || (!admin && (route.page === "login" || route.page === "settings"));
  if (needsLogin)
    return (
      <Login
        canGoBack={role === "public"}
        onSuccess={() => {
          signedIn();
          if (route.page === "login") navigate({ page: "settings", tab: "servers" });
        }}
      />
    );
  if (route.page === "login") return <RedirectHome />;

  const logout = () => {
    void api.logout().finally(() => {
      navigate({ page: "overview" });
      signedOut();
    });
  };

  return (
    <Layout route={route} admin={admin} onLogout={logout}>
      {route.page === "overview" && <Overview admin={admin} />}
      {route.page === "server" && <ServerDetail id={route.id} admin={admin} />}
      {route.page === "settings" && <Settings tab={route.tab} config={config} />}
    </Layout>
  );
}
