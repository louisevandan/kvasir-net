//! Exact benchmark prompts, loaded without teaching P4 what they mean.

pub fn load(requests: usize) -> Result<Vec<String>, String> {
    if let Ok(path) = std::env::var("P4_DRIVE_PROMPT_SET_FILE") {
        if std::env::var_os("P4_DRIVE_PROMPT_FILE").is_some() {
            return Err("P4_DRIVE_PROMPT_SET_FILE and P4_DRIVE_PROMPT_FILE are exclusive".into());
        }
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read prompt set {path}: {error}"))?;
        return parse_set(&source, requests)
            .map_err(|error| format!("cannot parse prompt set {path}: {error}"));
    }

    let prompt = match std::env::var("P4_DRIVE_PROMPT_FILE") {
        Ok(path) => std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {path}: {error}"))?,
        Err(_) => {
            std::env::var("P4_DRIVE_PROMPT").unwrap_or_else(|_| "simulated prompt".to_owned())
        }
    };
    if prompt.trim().is_empty() {
        return Err("prompt is empty".into());
    }
    Ok(vec![prompt])
}

fn parse_set(source: &str, requests: usize) -> Result<Vec<String>, String> {
    let prompts: Vec<String> = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if prompts.len() != requests {
        return Err(format!(
            "prompt set contains {} prompts, but the run requests {requests}",
            prompts.len()
        ));
    }
    if prompts.iter().any(|prompt| prompt.trim().is_empty()) {
        return Err("prompt set contains an empty prompt".into());
    }
    Ok(prompts)
}

#[cfg(test)]
mod tests {
    use super::parse_set;

    #[test]
    fn prompt_sets_are_exact_and_non_empty() {
        assert_eq!(
            parse_set(r#"["one","two"]"#, 2).expect("set"),
            ["one", "two"]
        );
        assert!(
            parse_set(r#"["one"]"#, 2)
                .unwrap_err()
                .contains("contains 1")
        );
        assert!(
            parse_set(r#"["one", " "]"#, 2)
                .unwrap_err()
                .contains("empty")
        );
    }
}
