// Browser smoke test against a running Worker: exercises every write the UI performs
// through the real cookie + Origin path. Usage:
//   MONI_BASE=http://127.0.0.1:8787 MONI_PASSWORD=dev-admin pnpm test:smoke
// Uses the Playwright-managed Chromium (`pnpm exec playwright-core install chromium`) unless
// CHROME_PATH points at another Chromium build.
import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const base = process.env.MONI_BASE ?? "http://127.0.0.1:8787";
const password = process.env.MONI_PASSWORD ?? "dev-admin";
const name = `smoke-${Date.now().toString(36)}`;

const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
const page = await browser.newPage({ viewport: { width: 1280, height: 860 } });
const failures = [];
page.on("pageerror", (e) => failures.push(`page error: ${e.message}`));
page.on("response", (r) => {
  const url = new URL(r.url());
  // /api/auth/me answers 401 before login by design.
  if (url.pathname.startsWith("/api/") && r.status() >= 400 && url.pathname !== "/api/auth/me")
    failures.push(`${r.request().method()} ${url.pathname} → ${r.status()}`);
});
// Confirmations are in-page alert dialogs; a native one would be a regression.
page.on("dialog", (d) => {
  failures.push(`native ${d.type()} dialog: ${d.message()}`);
  void d.dismiss();
});

const dialog = () => page.getByRole("dialog");
/** Answers an in-page confirmation by pressing its action button. */
const confirm = (action) => page.getByRole("alertdialog").getByRole("button", { name: action }).click();

const step = async (label, fn) => {
  await fn();
  console.log(`✓ ${label}`);
};

try {
  await step("public overview, then login", async () => {
    await page.goto(base);
    await page.waitForSelector("text=实时", { timeout: 10_000 });
    assert.equal(await page.getByRole("link", { name: "管理" }).count(), 0, "public view must not link to settings");
    await page.click("text=登录");
    await page.fill('input[type="password"]', password);
    await page.click('button[type="submit"]');
    await page.waitForSelector("main");
    await page.waitForSelector("text=实时", { timeout: 10_000 });
  });

  await step("create server (POST JSON)", async () => {
    await page.goto(`${base}/#/settings/servers`);
    await page.click("text=添加服务器");
    await page.fill('input[placeholder="例如 香港 01"]', name);
    await dialog().locator('button[type="submit"]').click();
    await page.waitForSelector("text=Token 只显示这一次");
    const token = await page.locator("[data-token]").textContent();
    assert.match(token ?? "", /^mn_[a-z0-9-]+_[A-Za-z0-9_-]{43}$/);
    await page.click("text=我已保存");
  });

  const row = () => page.locator("[data-server-row]", { hasText: name });

  await step("rename server (PATCH JSON)", async () => {
    await row().locator("text=重命名").click();
    await dialog().locator("input").fill(`${name}-r`);
    await dialog().locator('button[type="submit"]').click();
    await page.waitForSelector(`text=${name}-r`);
  });

  await step("rotate token (bodyless POST)", async () => {
    await row().locator("text=轮换 Token").click();
    await confirm("轮换");
    await page.waitForSelector("text=Token 只显示这一次");
    await page.click("text=我已保存");
  });

  await step("server detail renders", async () => {
    await row().locator("a").click();
    await page.waitForSelector("text=历史");
    await page.waitForSelector("text=Agent 尚未连接");
    await page.goBack();
    await page.waitForSelector(`text=${name}-r`);
  });

  await step("alert rule create / toggle / delete", async () => {
    await page.goto(`${base}/#/settings/alerts`);
    await page.click("text=添加规则");
    await dialog().locator('input[placeholder="CPU 过高"]').fill(`${name}-rule`);
    await dialog().locator('button[type="submit"]').click();
    const rule = page.locator("[data-rule-row]", { hasText: `${name}-rule` });
    await rule.getByRole("switch").click();
    await rule.locator("text=已停用").waitFor();
    await rule.getByRole("button", { name: "删除" }).click();
    await confirm("删除");
    await rule.waitFor({ state: "detached" });
  });

  await step("notification test (bodyless POST)", async () => {
    await page.goto(`${base}/#/settings/notify`);
    await page.click("text=发送测试通知");
    // Without notification secrets each channel reports "未启用"; with them, "发送成功" or an error.
    const result = page.getByText("未启用").or(page.getByText("发送成功")).first();
    await result.waitFor({ timeout: 10_000 });
  });

  await step("delete server (DELETE)", async () => {
    await page.goto(`${base}/#/settings/servers`);
    await row().locator("text=删除").click();
    await confirm("删除");
    await row().waitFor({ state: "detached" });
  });

  await step("logout (bodyless POST)", async () => {
    await page.getByRole("button", { name: "退出" }).click();
    await page.waitForSelector("a:has-text('登录')");
    await page.goto(`${base}/#/settings/servers`);
    await page.waitForSelector('input[type="password"]');
  });

  assert.deepEqual(failures, [], `unexpected errors:\n${failures.join("\n")}`);
  console.log("UI smoke passed");
} finally {
  await browser.close();
}
