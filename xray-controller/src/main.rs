use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

const CONTROL_SOCKET: &str = "/run/xray-control/control.sock";
const DOCKER_SOCKET: &str = "/var/run/docker.sock";
const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
const XRAY_ROLE_LABEL: &str = "org.xray.controller.role";
const XRAY_ROLE_VALUE: &str = "core";

#[derive(Debug)]
struct DockerResponse {
    status: u16,
    reason: String,
    body: Vec<u8>,
}

#[derive(Deserialize)]
struct ContainerInspect {
    #[serde(rename = "Config")]
    config: ContainerConfig,
    #[serde(rename = "State")]
    state: ContainerState,
}

#[derive(Deserialize)]
struct ContainerConfig {
    #[serde(rename = "Labels", default)]
    labels: HashMap<String, String>,
}

#[derive(Deserialize)]
struct ContainerState {
    #[serde(rename = "Running")]
    running: bool,
    #[serde(rename = "Restarting")]
    restarting: bool,
}

#[derive(Deserialize)]
struct ContainerSummary {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
}

#[derive(Deserialize)]
struct ExecCreated {
    #[serde(rename = "Id")]
    id: String,
}

#[derive(Deserialize)]
struct ExecInspect {
    #[serde(rename = "Running")]
    running: bool,
    #[serde(rename = "ExitCode")]
    exit_code: i32,
}

fn percent_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn decode_chunked(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    loop {
        let line_end = find_bytes(input, b"\r\n").ok_or("invalid chunked response")?;
        let size_text = std::str::from_utf8(&input[..line_end])
            .map_err(|_| "invalid chunk size")?
            .split(';')
            .next()
            .unwrap_or("");
        let size = usize::from_str_radix(size_text.trim(), 16)
            .map_err(|_| "invalid chunk size")?;
        input = &input[line_end + 2..];
        if size == 0 {
            return Ok(output);
        }
        if input.len() < size + 2 || &input[size..size + 2] != b"\r\n" {
            return Err("truncated chunked response".into());
        }
        output.extend_from_slice(&input[..size]);
        input = &input[size + 2..];
    }
}

fn docker_request(method: &str, path: &str, body: Option<&Value>) -> Result<DockerResponse, String> {
    let payload = match body {
        Some(value) => serde_json::to_vec(value).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    let mut stream = UnixStream::connect(DOCKER_SOCKET)
        .map_err(|error| format!("connect Docker API: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(95)))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;

    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: docker\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|error| format!("write Docker API request: {error}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("read Docker API response: {error}"))?;
    let header_end = find_bytes(&raw, b"\r\n\r\n").ok_or("malformed Docker API response")?;
    let headers = std::str::from_utf8(&raw[..header_end])
        .map_err(|_| "non-UTF-8 Docker API headers")?;
    let mut lines = headers.lines();
    let status_line = lines.next().ok_or("missing Docker API status")?;
    let mut status_parts = status_line.splitn(3, ' ');
    let _version = status_parts.next();
    let status = status_parts
        .next()
        .ok_or("missing Docker API status code")?
        .parse::<u16>()
        .map_err(|_| "invalid Docker API status code")?;
    let reason = status_parts.next().unwrap_or("").to_string();
    let is_chunked = lines.any(|line| {
        line.split_once(':')
            .map(|(name, value)| {
                name.eq_ignore_ascii_case("transfer-encoding")
                    && value.to_ascii_lowercase().contains("chunked")
            })
            .unwrap_or(false)
    });
    let body_bytes = &raw[header_end + 4..];
    let decoded = if is_chunked {
        decode_chunked(body_bytes)?
    } else {
        body_bytes.to_vec()
    };
    Ok(DockerResponse {
        status,
        reason,
        body: decoded,
    })
}

fn docker_error(response: &DockerResponse) -> String {
    let message = String::from_utf8_lossy(&response.body);
    format!(
        "Docker API returned {} {}: {}",
        response.status,
        response.reason,
        message.trim()
    )
}

fn inspect_container(id: &str) -> Result<ContainerInspect, String> {
    let response = docker_request("GET", &format!("/containers/{id}/json"), None)?;
    if response.status != 200 {
        return Err(docker_error(&response));
    }
    serde_json::from_slice(&response.body).map_err(|error| format!("decode container inspect: {error}"))
}

fn discover_target() -> Result<(String, String, String), String> {
    let self_id = env::var("HOSTNAME").map_err(|_| "HOSTNAME is unavailable")?;
    let self_info = inspect_container(&self_id)?;
    let project = self_info
        .config
        .labels
        .get(COMPOSE_PROJECT_LABEL)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or("controller lacks a Compose project label")?;

    let filters = json!({
        "label": [
            format!("{COMPOSE_PROJECT_LABEL}={project}"),
            format!("{XRAY_ROLE_LABEL}={XRAY_ROLE_VALUE}")
        ]
    });
    let encoded = percent_encode(&filters.to_string());
    let response = docker_request(
        "GET",
        &format!("/containers/json?all=1&filters={encoded}"),
        None,
    )?;
    if response.status != 200 {
        return Err(docker_error(&response));
    }
    let matches: Vec<ContainerSummary> = serde_json::from_slice(&response.body)
        .map_err(|error| format!("decode container list: {error}"))?;
    if matches.len() != 1 {
        return Err(format!(
            "expected exactly one container in Compose project {project} with label {XRAY_ROLE_LABEL}={XRAY_ROLE_VALUE}, found {}",
            matches.len()
        ));
    }
    let target = &matches[0];
    let display_name = target
        .names
        .first()
        .map(|name| name.trim_start_matches('/').to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| target.id.chars().take(12).collect());
    Ok((target.id.clone(), display_name, project))
}

fn fixed_config_test(target_id: &str) -> Result<(), String> {
    let body = json!({
        "AttachStdout": true,
        "AttachStderr": true,
        "Tty": false,
        "Cmd": [
            "/usr/local/bin/xray",
            "run",
            "-test",
            "-c",
            "/usr/local/etc/xray/config.json"
        ]
    });
    let response = docker_request(
        "POST",
        &format!("/containers/{target_id}/exec"),
        Some(&body),
    )?;
    if response.status != 201 {
        return Err(docker_error(&response));
    }
    let created: ExecCreated = serde_json::from_slice(&response.body)
        .map_err(|error| format!("decode exec creation: {error}"))?;
    if created.id.is_empty() {
        return Err("Docker API returned an empty exec ID".into());
    }

    let response = docker_request(
        "POST",
        &format!("/exec/{}/start", created.id),
        Some(&json!({"Detach": false, "Tty": false})),
    )?;
    if response.status != 200 {
        return Err(docker_error(&response));
    }
    let response = docker_request("GET", &format!("/exec/{}/json", created.id), None)?;
    if response.status != 200 {
        return Err(docker_error(&response));
    }
    let inspected: ExecInspect = serde_json::from_slice(&response.body)
        .map_err(|error| format!("decode exec inspect: {error}"))?;
    if inspected.running {
        return Err("fixed Xray configuration test did not finish".into());
    }
    if inspected.exit_code != 0 {
        return Err(format!(
            "fixed Xray configuration test failed with exit code {}",
            inspected.exit_code
        ));
    }
    Ok(())
}

fn restart(target_id: &str) -> Result<(), String> {
    let response = docker_request(
        "POST",
        &format!("/containers/{target_id}/restart?t=10"),
        None,
    )?;
    if response.status != 204 {
        return Err(docker_error(&response));
    }
    Ok(())
}

fn wait_running(target_id: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(info) = inspect_container(target_id)
            && info.state.running
            && !info.state.restarting
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "discovered container {} did not become running within {} seconds",
        &target_id[..target_id.len().min(12)],
        timeout.as_secs()
    ))
}

fn apply() -> Result<String, String> {
    let (target_id, display_name, project) = discover_target()?;
    let state = inspect_container(&target_id)?;
    if state.state.running && !state.state.restarting {
        fixed_config_test(&target_id)
            .map_err(|error| format!("pre-restart configuration test: {error}"))?;
    }
    restart(&target_id).map_err(|error| format!("restart discovered container: {error}"))?;
    wait_running(&target_id, Duration::from_secs(30))?;
    fixed_config_test(&target_id)
        .map_err(|error| format!("post-restart configuration test: {error}"))?;
    Ok(format!(
        "configuration validated, discovered container {display_name} in Compose project {project} restarted, post-check passed"
    ))
}

fn json_response(stream: &mut UnixStream, status: &str, ok: bool, message: &str) {
    let body = json!({"ok": ok, "message": message}).to_string() + "\n";
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn handle_connection(mut stream: UnixStream, apply_lock: Arc<Mutex<()>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(95)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while request.len() <= 8192 && find_bytes(&request, b"\r\n\r\n").is_none() {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(size) => request.extend_from_slice(&buffer[..size]),
            Err(_) => return,
        }
    }
    if request.len() > 8192 {
        json_response(&mut stream, "431 Request Header Fields Too Large", false, "request headers too large");
        return;
    }
    let header_end = match find_bytes(&request, b"\r\n\r\n") {
        Some(position) => position,
        None => {
            json_response(&mut stream, "400 Bad Request", false, "incomplete request");
            return;
        }
    };
    let headers = match std::str::from_utf8(&request[..header_end]) {
        Ok(value) => value,
        Err(_) => {
            json_response(&mut stream, "400 Bad Request", false, "invalid request");
            return;
        }
    };
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or("");
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    match request_line {
        "GET /health HTTP/1.1" => {
            json_response(&mut stream, "200 OK", true, "healthy");
        }
        "POST /apply HTTP/1.1" if content_length == 0 && request.len() == header_end + 4 => {
            let _guard = match apply_lock.try_lock() {
                Ok(guard) => guard,
                Err(TryLockError::WouldBlock) => {
                    json_response(&mut stream, "409 Conflict", false, "an apply operation is already running");
                    return;
                }
                Err(TryLockError::Poisoned(_)) => {
                    json_response(&mut stream, "500 Internal Server Error", false, "controller lock is unavailable");
                    return;
                }
            };
            match apply() {
                Ok(message) => {
                    eprintln!("apply completed: {message}");
                    json_response(&mut stream, "200 OK", true, &message);
                }
                Err(error) => {
                    eprintln!("apply failed: {error}");
                    json_response(&mut stream, "422 Unprocessable Entity", false, &error);
                }
            }
        }
        line if line.starts_with("POST /apply") => {
            json_response(&mut stream, "400 Bad Request", false, "only an empty, parameterless POST /apply is accepted");
        }
        _ => {
            json_response(&mut stream, "404 Not Found", false, "not found");
        }
    }
}

fn healthcheck() -> Result<(), String> {
    let mut stream = UnixStream::connect(CONTROL_SOCKET).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.starts_with(b"HTTP/1.1 200 OK\r\n") {
        Ok(())
    } else {
        Err("health endpoint did not return 200".into())
    }
}

fn run_server() -> Result<(), String> {
    if let Some(parent) = Path::new(CONTROL_SOCKET).parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if Path::new(CONTROL_SOCKET).exists() {
        fs::remove_file(CONTROL_SOCKET).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(CONTROL_SOCKET).map_err(|error| error.to_string())?;
    fs::set_permissions(CONTROL_SOCKET, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    eprintln!(
        "controller listening on {CONTROL_SOCKET}; target is uniquely discovered by Compose project and role label"
    );
    let apply_lock = Arc::new(Mutex::new(()));
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let lock = Arc::clone(&apply_lock);
                thread::spawn(move || handle_connection(stream, lock));
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

fn main() {
    let result = match env::args().collect::<Vec<_>>().as_slice() {
        [_] => run_server(),
        [_, command] if command == "healthcheck" => healthcheck(),
        _ => Err("no command-line arguments are accepted".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
