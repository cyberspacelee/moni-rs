import { useCallback, useEffect, useState } from "react";
import type { AppConfig } from "./api/generated";
import { Layout } from "./components/Layout";
import { Spinner } from "./components/ui";
import { api, UNAUTHORIZED_EVENT } from "./lib/api";
import { connectLive, disconnectLive, nudgeLive } from "./lib/live";
import { useRoute } from "./lib/router";
import { Login } from "./pages/Login";
import { Overview } from "./pages/Overview";
import { ServerDetail } from "./pages/ServerDetail";
import { Settings } from "./pages/Settings";

type Auth = "checking" | "in" | "out";

export function App() {
  const [auth, setAuth] = useState<Auth>("checking");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const route = useRoute();

  const signedIn = useCallback(() => {
    setAuth("in");
    api.config().then(setConfig, () => setConfig(null));
  }, []);

  useEffect(() => {
    api.me().then(signedIn, () => setAuth("out"));
    const onUnauthorized = () => setAuth("out");
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, [signedIn]);

  useEffect(() => {
    if (auth !== "in") return;
    connectLive();
    const onVisible = () => document.visibilityState === "visible" && nudgeLive();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      disconnectLive();
    };
  }, [auth]);

  if (auth === "checking")
    return (
      <div className="grid min-h-dvh place-items-center">
        <Spinner />
      </div>
    );
  if (auth === "out") return <Login onSuccess={signedIn} />;

  const logout = () => {
    void api.logout().finally(() => setAuth("out"));
  };

  return (
    <Layout route={route} onLogout={logout} version={config?.version}>
      {route.page === "overview" && <Overview />}
      {route.page === "server" && <ServerDetail id={route.id} />}
      {route.page === "settings" && <Settings tab={route.tab} config={config} />}
    </Layout>
  );
}
