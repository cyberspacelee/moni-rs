import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import type { AppConfig } from "@/api/generated";
import { PageSpinner } from "@/components/common";
import { Layout } from "@/components/Layout";
import { api, UNAUTHORIZED_EVENT } from "@/lib/api";
import { connectLive, disconnectLive, nudgeLive } from "@/lib/live";
import { navigate, useRoute } from "@/lib/router";
import { Overview } from "@/pages/Overview";

// The public overview is the landing page; charts (uPlot), forms and admin screens load on demand.
const Login = lazy(() => import("@/pages/Login").then((m) => ({ default: m.Login })));
const ServerDetail = lazy(() => import("@/pages/ServerDetail").then((m) => ({ default: m.ServerDetail })));
const Settings = lazy(() => import("@/pages/Settings").then((m) => ({ default: m.Settings })));

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

  if (role === "checking") return <PageSpinner />;

  const admin = role === "admin";
  const needsLogin = role === "locked" || (!admin && (route.page === "login" || route.page === "settings"));
  if (needsLogin)
    return (
      <Suspense fallback={<PageSpinner />}>
        <Login
          canGoBack={role === "public"}
          onSuccess={() => {
            signedIn();
            if (route.page === "login") navigate({ page: "settings", tab: "servers" });
          }}
        />
      </Suspense>
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
      <Suspense fallback={<PageSpinner />}>
        {route.page === "overview" && <Overview admin={admin} />}
        {route.page === "server" && <ServerDetail id={route.id} admin={admin} />}
        {route.page === "settings" && <Settings tab={route.tab} config={config} />}
      </Suspense>
    </Layout>
  );
}
