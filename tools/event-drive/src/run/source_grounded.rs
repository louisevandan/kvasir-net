//! OUTER-owned, opt-in calculation for a source-record task. The user task
//! chooses record identities; the source text supplies the quantities. The sealed
//! benchmark oracle is never an input to this module.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseProcessor {
    EngineeringPowerV1,
}

#[derive(Debug)]
struct SourceRecord {
    revision: u64,
    current_a: u64,
    resistance_milliohms: u64,
    duration_hours: u64,
    pressure_kpa: u64,
    alarm_threshold_kpa: u64,
    temperature_measured: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelRow {
    id: String,
    revision: u64,
    #[serde(rename = "power_mW")]
    power_m_w: u64,
    #[serde(rename = "energy_mWh")]
    energy_m_wh: u64,
    pressure_alarm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelAnswer {
    rows: Vec<ModelRow>,
    temperature_measured: bool,
}

pub(super) const TASK_SUFFIX: &str = "Compute power in milliwatts as current_A squared times resistance_milliohms, and energy in milliwatt-hours as power_mW times duration_hours. Return JSON with keys \"rows\" and \"temperature_measured\". Each row must contain \"id\", \"revision\", \"power_mW\", \"energy_mWh\", and boolean \"pressure_alarm\". \"temperature_measured\" must state whether those records contain a measured temperature. Use integer arithmetic; do not infer a temperature or a pressure/heat causal relation.";

fn source_record(line: &str) -> Result<(String, SourceRecord), &'static str> {
    let (id, rest) = line
        .strip_prefix('[')
        .and_then(|line| line.split_once("] Station "))
        .ok_or("source record identity is malformed")?;
    if !valid_id(id) {
        return Err("source record identity is malformed");
    }
    let (station, rest) = rest
        .split_once("; revision ")
        .ok_or("source record station is malformed")?;
    if station.is_empty() || !station.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("source record station is malformed");
    }
    station
        .parse::<u64>()
        .map_err(|_| "source record station is malformed")?;
    let (revision, rest) = rest
        .split_once(". Measured RMS current: ")
        .ok_or("source record revision is malformed")?;
    let (current_a, rest) = rest
        .split_once(" A. Isolated conductor resistance: ")
        .ok_or("source record current is malformed")?;
    let (resistance_milliohms, rest) = rest
        .split_once(" milliohms. Operating duration: ")
        .ok_or("source record resistance is malformed")?;
    let (duration_hours, rest) = rest
        .split_once(" hours. Inlet pressure: ")
        .ok_or("source record duration is malformed")?;
    let (pressure_kpa, rest) = rest
        .split_once(" kPa. Pressure alarm threshold: ")
        .ok_or("source record pressure is malformed")?;
    let (alarm_threshold_kpa, rest) = rest
        .split_once(" kPa; equality is not an exceedance. ")
        .ok_or("source record alarm threshold is malformed")?;
    let temperature_measured = match rest {
        "No temperature measurement is recorded." | "Temperature was not measured." => false,
        _ => return Err("source record temperature status is malformed"),
    };
    let number = |value: &str| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("source record quantity is malformed");
        }
        value
            .parse::<u64>()
            .map_err(|_| "source record quantity is malformed")
    };
    Ok((
        id.to_owned(),
        SourceRecord {
            revision: number(revision)?,
            current_a: number(current_a)?,
            resistance_milliohms: number(resistance_milliohms)?,
            duration_hours: number(duration_hours)?,
            pressure_kpa: number(pressure_kpa)?,
            alarm_threshold_kpa: number(alarm_threshold_kpa)?,
            temperature_measured,
        },
    ))
}

fn valid_id(id: &str) -> bool {
    id.len() == 6 && id.starts_with('R') && id[1..].bytes().all(|byte| byte.is_ascii_digit())
}

fn source_and_selection(prompt: &str) -> Result<(BTreeMap<String, SourceRecord>, Vec<String>), &'static str> {
    let user = prompt
        .split_once("<|im_start|>user\n")
        .and_then(|(_, rest)| rest.split_once("<|im_end|>"))
        .map(|(user, _)| user)
        .ok_or("source user message is missing")?;
    let (source, task) = user
        .rsplit_once("\n\n")
        .ok_or("source task boundary is missing")?;
    let header = source.lines().next().ok_or("source header is missing")?;
    let (case, count) = header
        .strip_prefix("Case ")
        .and_then(|header| header.split_once(": "))
        .ok_or("source record count is malformed")?;
    if case.is_empty() || !case.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("source case identity is malformed");
    }
    let count = count
        .strip_suffix(" archived records.")
        .filter(|count| !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|count| count.parse::<usize>().ok())
        .ok_or("source record count is malformed")?;
    if count == 0 || count > 10_000 {
        return Err("source record count is out of bounds");
    }
    let mut records = BTreeMap::new();
    for line in source.lines().skip(1) {
        let (id, record) = source_record(line.trim_end_matches('\r'))?;
        if records.insert(id, record).is_some() {
            return Err("source record identity is duplicated");
        }
    }
    if records.len() != count {
        return Err("source record count differs");
    }
    let ids = task
        .strip_prefix("Connect the source facts for ")
        .and_then(|task| task.split_once(", in that order. "))
        .and_then(|(ids, suffix)| (suffix == TASK_SUFFIX).then_some(ids))
        .ok_or("source calculation contract differs")?;
    let selected = ids.split(", ").map(str::to_owned).collect::<Vec<_>>();
    if selected.is_empty() || selected.len() > 256 {
        return Err("source selection size is out of bounds");
    }
    let mut unique = BTreeSet::new();
    for id in &selected {
        if !valid_id(id) || !unique.insert(id) || !records.contains_key(id) {
            return Err("source selection identity is invalid");
        }
    }
    Ok((records, selected))
}

fn parse_model_answer(raw: &str) -> Result<ModelAnswer, &'static str> {
    let text = raw.trim();
    let text = if let Some(inner) = text.strip_prefix("```json\n") {
        inner
            .strip_suffix("\n```")
            .ok_or("model JSON fence is malformed")?
    } else {
        text
    };
    serde_json::from_str(text).map_err(|_| "model answer is not the required JSON object")
}

pub fn process(
    processor: ResponseProcessor,
    prompt: &str,
    raw_response: &str,
) -> Result<String, &'static str> {
    match processor {
        ResponseProcessor::EngineeringPowerV1 => engineering_power(prompt, raw_response),
    }
}

fn engineering_power(prompt: &str, raw_response: &str) -> Result<String, &'static str> {
    let (records, selected) = source_and_selection(prompt)?;
    let model = parse_model_answer(raw_response)?;
    if model.rows.len() != selected.len() {
        return Err("model selection size differs from source task");
    }
    let mut output_rows = Vec::with_capacity(selected.len());
    for (row, id) in model.rows.iter().zip(&selected) {
        if row.id != *id {
            return Err("model selection identity or order differs from source task");
        }
        let source = records.get(id).expect("validated source selection");
        if row.revision != source.revision {
            return Err("model revision differs from source record");
        }
        let power = source
            .current_a
            .checked_mul(source.current_a)
            .and_then(|value| value.checked_mul(source.resistance_milliohms))
            .ok_or("source power arithmetic overflow")?;
        let energy = power
            .checked_mul(source.duration_hours)
            .ok_or("source energy arithmetic overflow")?;
        // These typed fields are required in the model's schema, but their
        // guessed values have no authority. Keep the read explicit so a
        // compiler warning cannot hide an accidentally unused input field.
        let _model_guess = (row.power_m_w, row.energy_m_wh, row.pressure_alarm);
        output_rows.push(serde_json::json!({
            "id": id,
            "revision": source.revision,
            "power_mW": power,
            "energy_mWh": energy,
            "pressure_alarm": source.pressure_kpa > source.alarm_threshold_kpa,
        }));
    }
    let source_temperature = selected.iter().any(|id| {
        records
            .get(id)
            .expect("validated source selection")
            .temperature_measured
    });
    if model.temperature_measured != source_temperature {
        return Err("model temperature fact differs from source records");
    }
    serde_json::to_string(&serde_json::json!({
        "rows": output_rows,
        "temperature_measured": source_temperature,
    }))
    .map_err(|_| "source result serialization failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> String {
        let records = [
            "[R00002] Station 7; revision 6. Measured RMS current: 28 A. Isolated conductor resistance: 52 milliohms. Operating duration: 10 hours. Inlet pressure: 85 kPa. Pressure alarm threshold: 120 kPa; equality is not an exceedance. Temperature was not measured.",
            "[R00196] Station 13; revision 2. Measured RMS current: 58 A. Isolated conductor resistance: 52 milliohms. Operating duration: 11 hours. Inlet pressure: 121 kPa. Pressure alarm threshold: 120 kPa; equality is not an exceedance. No temperature measurement is recorded.",
        ];
        format!(
            "<|im_start|>user\nCase 5: 2 archived records.\n{}\n{}\n\nConnect the source facts for R00002, R00196, in that order. {TASK_SUFFIX}<|im_end|>",
            records[0], records[1]
        )
    }

    fn raw() -> &'static str {
        r#"{"rows":[{"id":"R00002","revision":6,"power_mW":1,"energy_mWh":2,"pressure_alarm":true},{"id":"R00196","revision":2,"power_mW":3,"energy_mWh":4,"pressure_alarm":false}],"temperature_measured":false}"#
    }

    #[test]
    fn source_quantities_override_only_model_guesses() {
        let actual = process(ResponseProcessor::EngineeringPowerV1, &prompt(), raw()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&actual).unwrap();
        assert_eq!(value["rows"][0]["power_mW"], 28 * 28 * 52);
        assert_eq!(value["rows"][0]["energy_mWh"], 28 * 28 * 52 * 10);
        assert_eq!(value["rows"][0]["pressure_alarm"], false);
        assert_eq!(value["rows"][1]["power_mW"], 58 * 58 * 52);
        assert_eq!(value["rows"][1]["energy_mWh"], 58 * 58 * 52 * 11);
        assert_eq!(value["rows"][1]["pressure_alarm"], true);
        let fenced = format!("```json\n{raw}\n```", raw = raw());
        assert_eq!(process(ResponseProcessor::EngineeringPowerV1, &prompt(), &fenced).unwrap(), actual);
    }

    #[test]
    fn source_and_model_identity_must_both_survive() {
        for (prompt, raw, error) in [
            (prompt().replace("Case 5: 2", "Case 5: 3"), raw().to_owned(), "source record count differs"),
            (prompt().replace("[R00196]", "[R00002]"), raw().to_owned(), "source record identity is duplicated"),
            (prompt(), raw().replace("\"revision\":2", "\"revision\":3"), "model revision differs from source record"),
            (prompt(), raw().replace("\"id\":\"R00196\"", "\"id\":\"R00002\""), "model selection identity or order differs from source task"),
            (prompt(), raw().replace("\"temperature_measured\":false", "\"temperature_measured\":true"), "model temperature fact differs from source records"),
            (prompt(), format!("```json\n{}", raw()), "model JSON fence is malformed"),
        ] {
            assert_eq!(
                process(ResponseProcessor::EngineeringPowerV1, &prompt, &raw),
                Err(error)
            );
        }
    }
}
