//! llama-server endpoint parsing.

pub(crate) fn endpoint(value: &str) -> Result<(String, u16), Box<dyn std::error::Error>> {
    let value = value.strip_prefix("http://").unwrap_or(value);
    let (host, port) = value
        .rsplit_once(':')
        .ok_or("llama endpoint must be host:port")?;
    Ok((host.into(), port.parse()?))
}
