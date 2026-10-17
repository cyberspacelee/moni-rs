import { type FormEvent, useState } from "react";
import { Button, Card, ErrorText, Field, Input } from "../components/ui";
import { ApiError, api, errorText } from "../lib/api";

export function Login({ onSuccess }: { onSuccess: () => void }) {
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
    <div className="grid min-h-dvh place-items-center px-4">
      <Card className="w-full max-w-sm p-6">
        <form onSubmit={submit} className="space-y-4">
          <div>
            <h1 className="text-lg font-semibold">moni-rs</h1>
            <p className="text-sm text-zinc-500">登录以查看服务器状态</p>
          </div>
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
          <Button type="submit" variant="primary" className="w-full" disabled={busy || !password}>
            {busy ? "登录中…" : "登录"}
          </Button>
        </form>
      </Card>
    </div>
  );
}
