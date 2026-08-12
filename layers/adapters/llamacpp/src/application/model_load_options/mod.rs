//! Model-load option filtering for a process-owned stock llama-server.

use serde_json::Value;

pub(crate) fn require_process_start_compatible(
    stage_plan: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = serde_json::from_str::<Value>(stage_plan)?;
    let plan = plan.as_object().ok_or("stage_plan must be a JSON object")?;
    if plan.contains_key("load_options") {
        return Err(
            "stock llama-server is already process-owned and cannot apply model-load options; start a concrete runtime with those options"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::require_process_start_compatible;

    #[test]
    fn accepts_a_binding_without_process_start_options() {
        assert!(require_process_start_compatible(r#"{"placement":"existing"}"#).is_ok());
    }

    #[test]
    fn rejects_options_that_a_running_process_cannot_apply() {
        let error = require_process_start_compatible(
            r#"{"load_options":{"flash_attention":"enabled","mmap":false}}"#,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot apply model-load options")
        );
    }
}
