use moni_protocol::api::Appearance;
use serde::Deserialize;
use worker::*;

use super::Hub;
use crate::error;

#[derive(Deserialize)]
struct MetaRow {
    v: String,
}

impl Hub {
    pub(super) fn appearance(&self) -> Result<Appearance> {
        let rows = self
            .state
            .storage()
            .sql()
            .exec("SELECT v FROM meta WHERE k = 'appearance'", None)?
            .to_array::<MetaRow>()?;
        // A row from an older shape must not lock visitors out of the dashboard.
        Ok(rows
            .first()
            .and_then(|row| serde_json::from_str(&row.v).ok())
            .unwrap_or_default())
    }

    pub(super) async fn set_appearance(&self, mut req: Request) -> Result<Response> {
        let Ok(appearance) = req.json::<Appearance>().await else {
            return error::bad_request("Expected a valid appearance");
        };
        if !appearance.valid() {
            return error::bad_request("Theme id must be 1-32 lowercase letters, digits or '-'");
        }
        self.state.storage().sql().exec(
            "INSERT INTO meta (k,v) VALUES ('appearance', ?)
            ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            vec![serde_json::to_string(&appearance)?.into()],
        )?;
        Response::from_json(&appearance)
    }
}
