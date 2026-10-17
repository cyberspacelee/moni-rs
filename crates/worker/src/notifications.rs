use std::time::Duration;

use futures_util::future::{Either, join, select};
use moni_protocol::api::AlertTestResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::*;

#[derive(Serialize, Deserialize)]
pub(crate) struct Notification {
    pub event: String,
    pub rule: Value,
    pub server: Value,
    pub value: Option<f64>,
    pub ts_ms: i64,
}

async fn post(url: &str, body: Value) -> String {
    let result = async {
        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;
        let req = Request::new_with_init(
            url,
            RequestInit::new()
                .with_method(Method::Post)
                .with_headers(headers)
                .with_redirect(RequestRedirect::Manual)
                .with_body(Some(body.to_string().into())),
        )?;
        let controller = AbortController::default();
        let signal = controller.signal();
        let fetch = Fetch::Request(req);
        let request = async {
            let response = fetch.send_with_signal(&signal).await?;
            let status = response.status_code();
            // Release the connection even if a receiver streams an unbounded response body.
            if let ResponseBody::Stream(stream) = response.body() {
                let _ = wasm_bindgen_futures::JsFuture::from(stream.cancel()).await;
            }
            Ok::<_, Error>(if (200..300).contains(&status) {
                "ok".into()
            } else {
                format!("HTTP {status}")
            })
        };
        match select(
            Box::pin(request),
            Box::pin(Delay::from(Duration::from_secs(5))),
        )
        .await
        {
            Either::Left((result, _)) => result,
            Either::Right(_) => {
                controller.abort();
                Ok("timeout".into())
            }
        }
    }
    .await;
    // Fetch errors can contain the URL, which includes the Telegram credential.
    result.unwrap_or_else(|_: Error| "request failed".into())
}

pub(crate) async fn send(env: Env, notice: Notification) -> AlertTestResponse {
    let telegram = async {
        let (Ok(token), Ok(chat)) = (env.secret("TG_BOT_TOKEN"), env.secret("TG_CHAT_ID")) else {
            return "disabled".into();
        };
        let title = if notice.event == "resolved" {
            "✅ 恢复"
        } else {
            "🔴 告警"
        };
        let text = format!(
            "{title}\n服务器：{}\n规则：{}\n当前值：{}\n时间：{}",
            notice
                .server
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("测试"),
            notice
                .rule
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("通知测试"),
            notice
                .value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "离线/无指标".into()),
            Date::new(DateInit::Millis(notice.ts_ms.max(0) as u64))
        );
        post(
            &format!("https://api.telegram.org/bot{token}/sendMessage"),
            json!({"chat_id": chat.to_string(), "text": text}),
        )
        .await
    };
    let webhook = async {
        let Ok(url) = env.secret("WEBHOOK_URL") else {
            return "disabled".into();
        };
        post(&url.to_string(), json!(notice)).await
    };
    let (telegram, webhook) = join(telegram, webhook).await;
    for (channel, result) in [("telegram", &telegram), ("webhook", &webhook)] {
        if result != "ok" && result != "disabled" {
            console_warn!("Alert notification {}: {}", channel, result);
        }
    }
    AlertTestResponse { telegram, webhook }
}
