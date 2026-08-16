//! Facts about the machine this agent runs on.
//!
//! Deliberately thin. What an agent can say for certain without a backend is
//! how many cores it has, what it is built for, and which adapters it can
//! serve — and the last of those is the one a placement actually needs, since
//! it decides whether a node can be created here at all.
//!
//! Device capability belongs to a backend and is reported by one. Guessing at
//! it here would put a hardware probe in the communication layer, which is
//! precisely the dependency this layer exists without.

/// A snapshot, as JSON text. Opaque to everything that carries it — only
/// whoever composes placements reads it.
pub fn snapshot(adapters: &[String]) -> String {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(0);
    let mut names = String::new();
    for (index, adapter) in adapters.iter().enumerate() {
        if index > 0 {
            names.push(',');
        }
        names.push('"');
        names.push_str(&escape(adapter));
        names.push('"');
    }
    format!(
        r#"{{"os":"{}","arch":"{}","cores":{cores},"adapters":[{names}]}}"#,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Only what a JSON string needs. Adapter names come from a registration in
/// this process rather than from the wire, but a name is still text and text
/// that breaks the snapshot would be read as a truncated one.
fn escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            control if control.is_control() => vec![' '],
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
mod tests;
