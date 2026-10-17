use moni_protocol::api::AlertRulePatch;

fn main() -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string(&[
            AlertRulePatch {
                threshold: Some(95.0),
                ..Default::default()
            },
            AlertRulePatch {
                server_id: Some(None),
                recover_threshold: Some(None),
                ..Default::default()
            },
        ])?
    );
    Ok(())
}
