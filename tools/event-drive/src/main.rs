mod run;

use run::RunConfig;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = PathBuf::from(std::env::args().nth(1).ok_or(USAGE)?);
    let artifact_path = PathBuf::from(std::env::args().nth(2).ok_or(USAGE)?);
    let bytes = std::fs::read(&config_path)?;
    let config: RunConfig = serde_json::from_slice(&bytes)?;
    let artifact = run::execute(config).await?;
    if let Some(parent) = artifact_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&artifact_path, serde_json::to_vec_pretty(&artifact)?)?;
    println!(
        "P4_EVENT_GATE_RESULT passed={} artifact={}",
        artifact.passed,
        artifact_path.display()
    );
    if !artifact.passed {
        std::process::exit(1);
    }
    Ok(())
}

const USAGE: &str = "usage: p4-event-drive CONFIG.json ARTIFACT.json";
