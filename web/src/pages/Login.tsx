import { ChevronLeftIcon, LockIcon } from "lucide-react";
import { type FormEvent, useState } from "react";
import { Logo } from "@/components/brand";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { ApiError, api, errorText } from "@/lib/api";
import { href } from "@/lib/router";
import { cn } from "@/lib/utils";

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
          className={cn(buttonVariants({ variant: "ghost" }), "absolute top-[max(1rem,env(safe-area-inset-top))] left-4 text-ink-2")}
        >
          <ChevronLeftIcon data-icon="inline-start" />
          总览
        </a>
      )}
      <div className="w-full max-w-sm">
        <div className="mb-8 flex flex-col items-center text-center">
          <Logo className="size-14" />
          <h1 className="mt-5 text-xl font-semibold tracking-tight">登录 moni</h1>
          <p className="mt-1 text-sm text-muted-foreground">管理服务器、告警规则与通知</p>
        </div>
        <Card>
          <CardContent className="sm:px-6">
            <form onSubmit={submit}>
              <FieldGroup>
                <Field data-invalid={!!error || undefined}>
                  <FieldLabel htmlFor="password">管理员密码</FieldLabel>
                  <Input
                    id="password"
                    type="password"
                    autoFocus
                    autoComplete="current-password"
                    aria-invalid={!!error || undefined}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                  {error && <FieldError>{error}</FieldError>}
                </Field>
                <Button type="submit" size="lg" className="w-full" disabled={busy || !password}>
                  {busy ? <Spinner data-icon="inline-start" /> : <LockIcon data-icon="inline-start" />}
                  {busy ? "登录中…" : "登录"}
                </Button>
              </FieldGroup>
            </form>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
