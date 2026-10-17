import { type FormEvent, useState } from "react";
import { IconBack, IconLock, Logo } from "../components/icons";
import { Button, ErrorText, Field, Input } from "../components/ui";
import { ApiError, api, errorText } from "../lib/api";
import { href } from "../lib/router";

export function Login({ onSuccess, canGoBack }: { onSuccess: () => void; canGoBack: boolean }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api.login(password);
      onSuccess();
    } catch (err) {
      setError(
        err instanceof ApiError && err.status === 401
          ? "密码错误"
          : err instanceof ApiError && err.status === 429
            ? "尝试次数过多，请稍后再试"
            : err instanceof ApiError && err.code === "not_configured"
              ? "服务端未配置 ADMIN_PASSWORD / AUTH_SECRET"
              : errorText(err),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="relative grid min-h-dvh place-items-center px-4 py-10">
      {canGoBack && (
        <a
          href={href({ page: "overview" })}
          className="absolute top-[max(1rem,env(safe-area-inset-top))] left-4 inline-flex h-8 items-center gap-1 rounded-md px-1.5 text-sm leading-none text-ink-2 hover:bg-surface-2 hover:text-ink"
        >
          <IconBack size={16} />
          总览
        </a>
      )}
      <div className="w-full max-w-sm">
        <div className="mb-8 flex flex-col items-center text-center">
          <Logo className="size-14" />
          <h1 className="mt-5 text-xl font-semibold tracking-tight">登录 moni</h1>
          <p className="mt-1 text-sm text-ink-3">管理服务器、告警规则与通知</p>
        </div>
        <form onSubmit={submit} className="space-y-4 rounded-[10px] border border-line bg-surface p-5 sm:p-6">
          <Field label="管理员密码">
            <Input
              type="password"
              autoFocus
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </Field>
          <ErrorText>{error}</ErrorText>
          <Button type="submit" variant="primary" className="h-10 w-full" disabled={busy || !password}>
            <IconLock size={16} />
            {busy ? "登录中…" : "登录"}
          </Button>
        </form>
      </div>
    </div>
  );
}
