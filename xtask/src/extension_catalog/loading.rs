//! MIT port of the build-time activity loader in Hunk's extension directory.
use super::{activity, index_activity};
use anyhow::{Result, bail};
use serde_json::Value;
use std::time::Duration;

pub(super) fn load(payload: &Value) -> Result<Vec<Value>> {
    let entries = payload
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("load expects an array of listings"))?;
    for entry in entries {
        if entry.get("repo").and_then(Value::as_str).is_none() {
            bail!("each listing requires a repository string");
        }
    }
    let token = std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|value| !value.is_empty());
    let fetch = |path: &str, timeout: Duration| -> Result<Value> {
        fetch_json(
            &format!("https://api.github.com{path}"),
            token.as_deref(),
            timeout,
        )
    };
    let (entries, warnings) = resolve(entries, "workdeck-extension", &fetch);
    for warning in warnings {
        eprintln!("{warning}");
    }
    Ok(entries)
}

#[derive(Debug)]
struct HttpStatus(u16);

impl std::fmt::Display for HttpStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "HTTP {}", self.0)
    }
}

impl std::error::Error for HttpStatus {}

fn fetch_json(url: &str, token: Option<&str>, timeout: Duration) -> Result<Value> {
    let started = std::time::Instant::now();
    let mut url = url::Url::parse(url)?;
    let mut token = token.filter(|token| !token.is_empty());
    for redirects in 0..=20 {
        let remaining = timeout
            .checked_sub(started.elapsed())
            .ok_or_else(|| anyhow::anyhow!("request deadline exceeded"))?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(remaining))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .into();
        let mut request = agent
            .get(url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "workdeck-extension-directory");
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let mut response = request.call()?;
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308)
            && let Some(location) = response.headers().get("location")
        {
            if redirects == 20 {
                bail!("too many redirects");
            }
            let next = url.join(location.to_str()?)?;
            if !matches!(next.scheme(), "http" | "https")
                || !next.username().is_empty()
                || next.password().is_some()
            {
                bail!("unsupported redirect destination");
            }
            if next.origin() != url.origin() {
                token = None;
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(HttpStatus(response.status().as_u16()).into());
        }
        return Ok(serde_json::from_str(
            &response.body_mut().read_to_string()?,
        )?);
    }
    unreachable!("redirect limit returns before leaving loop")
}

fn resolve(
    entries: &[Value],
    topic: &str,
    fetch: &(impl Fn(&str, Duration) -> Result<Value> + Sync),
) -> (Vec<Value>, Vec<String>) {
    let query = format!("topic%3A{topic}%20is%3Apublic");
    let mut warnings = Vec::new();
    let tagged = match fetch(
        &format!("/search/repositories?q={query}&per_page=100"),
        Duration::from_secs(8),
    ) {
        Ok(payload) => index_activity(&payload),
        Err(error) => {
            // Never log transport error details that might contain credentials.
            warnings.push(match error.downcast_ref::<HttpStatus>() {
                Some(HttpStatus(status)) => format!("Extension directory: GitHub topic search returned {status}; rendering without stars."),
                None => "Extension directory: GitHub topic search failed; rendering without stars.".into(),
            });
            Default::default()
        }
    };
    let missing: Vec<_> = entries
        .iter()
        .filter(|entry| !tagged.contains_key(&entry["repo"].as_str().unwrap().to_lowercase()))
        .collect();
    if !tagged.is_empty() && !missing.is_empty() {
        warnings.push(format!(
            "Extension directory: {} {} listed but not tagged `{topic}`.",
            missing
                .iter()
                .map(|entry| entry["repo"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join(", "),
            if missing.len() == 1 { "is" } else { "are" }
        ));
    }
    let direct: std::collections::BTreeMap<_, _> = std::thread::scope(|scope| {
        let handles: Vec<_> = missing
            .iter()
            .map(|entry| {
                let repo = entry["repo"].as_str().unwrap();
                scope.spawn(move || {
                    let data = fetch(&format!("/repos/{repo}"), Duration::from_secs(5))
                        .map(|value| activity(&value))
                        .unwrap_or_else(|_| serde_json::json!({}));
                    (repo, data)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("directory fetch worker panicked"))
            .collect()
    });
    let result = entries
        .iter()
        .map(|entry| {
            let repo = entry["repo"].as_str().unwrap();
            let mut entry = entry.clone();
            if let Some(metadata) = tagged
                .get(&repo.to_lowercase())
                .or_else(|| direct.get(repo))
            {
                entry
                    .as_object_mut()
                    .unwrap()
                    .extend(metadata.as_object().unwrap().clone());
            }
            entry
        })
        .collect();
    (result, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Barrier, Mutex};

    #[test]
    fn topic_http_status_warning_preserves_status_and_direct_fallback() {
        for status in [302, 304, 403, 404, 429, 503] {
            let (entries, warnings) = resolve(
                &[json!({"repo":"owner/repo"})],
                "workdeck-extension",
                &|path, _| {
                    if path.starts_with("/search/") {
                        Err(HttpStatus(status).into())
                    } else {
                        Ok(json!({"stargazers_count":2}))
                    }
                },
            );
            assert_eq!(entries, [json!({"repo":"owner/repo","stars":2})]);
            assert_eq!(
                warnings,
                [format!(
                    "Extension directory: GitHub topic search returned {status}; rendering without stars."
                )]
            );
        }
    }

    #[test]
    fn http_transport_deadline_interrupts_a_silent_server() {
        use std::net::TcpListener;
        use std::sync::mpsc;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/silent", listener.local_addr().unwrap());
        let (release, released) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Hold the socket open without sending headers. Release
                        // only after the client returns, with a safety ceiling.
                        let released_by_client =
                            released.recv_timeout(Duration::from_secs(5)).is_ok();
                        drop(stream);
                        return released_by_client;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            }
        });
        let started = std::time::Instant::now();
        let result = fetch_json(&url, None, Duration::from_millis(100));
        let elapsed = started.elapsed();
        let _ = release.send(());
        assert!(
            server.join().unwrap(),
            "server safety ceiling fired before client deadline"
        );
        let error = result.unwrap_err();
        assert!(
            matches!(
                error.downcast_ref::<ureq::Error>(),
                Some(ureq::Error::Timeout(_))
            ),
            "unexpected failure: {error}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "client exceeded deadline tolerance: {elapsed:?}"
        );
    }

    #[test]
    fn http_transport_sends_headers_and_rejects_bad_responses() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for (token, status, body, success) in [
            (None, "200 OK", r#"{"stars":7}"#, true),
            (Some(""), "200 OK", "null", true),
            (Some("fixture-token"), "200 OK", "[]", true),
            (None, "503 Service Unavailable", "{}", false),
            (None, "302 Found", "{}", false),
            (None, "304 Not Modified", "{}", false),
            (None, "404 Not Found", "{}", false),
            (None, "200 OK", "invalid JSON", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = format!("http://{}/repos/owner/repo", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "HTTP client did not connect"
                            );
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("accept failed: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                String::from_utf8(request).unwrap()
            });
            let result = fetch_json(&url, token, Duration::from_secs(5));
            let request = server.join().unwrap().to_lowercase();
            assert_eq!(result.is_ok(), success);
            if !status.starts_with("200") {
                assert_eq!(
                    result
                        .as_ref()
                        .unwrap_err()
                        .downcast_ref::<HttpStatus>()
                        .map(|status| status.0),
                    Some(status[..3].parse().unwrap())
                );
            }
            if success {
                assert_eq!(
                    result.unwrap(),
                    serde_json::from_str::<Value>(body).unwrap()
                );
            }
            assert!(request.starts_with("get /repos/owner/repo http/1.1\r\n"));
            assert!(request.contains("accept: application/vnd.github+json\r\n"));
            assert!(request.contains("user-agent: workdeck-extension-directory\r\n"));
            if token == Some("fixture-token") {
                assert!(request.contains("authorization: bearer fixture-token\r\n"));
            } else {
                assert!(!request.contains("authorization:"));
            }
        }
    }

    #[test]
    fn merged_entries_match_both_pinned_loader_captures() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-loading.json"
        ))
        .unwrap();
        for capture in fixture["captures"].as_array().unwrap() {
            for case in capture["cases"].as_array().unwrap() {
                let entries = case["input"].as_array().unwrap();
                let mode = case["mode"].as_str().unwrap();
                let requests = Mutex::new(Vec::new());
                let (result, warnings) = resolve(entries, "hunk-extension", &|path, _| {
                    requests
                        .lock()
                        .unwrap()
                        .push(format!("https://api.github.com{path}"));
                    if path.starts_with("/search/") {
                        if mode == "failed-topic" {
                            bail!("offline")
                        }
                        return Ok(json!({"items": if mode == "partial-topic" {
                            vec![json!({"full_name":entries[0]["repo"].as_str().unwrap().to_uppercase(),"stargazers_count":0})]
                        } else { vec![] }}));
                    }
                    let index = entries
                        .iter()
                        .position(|entry| {
                            path == format!("/repos/{}", entry["repo"].as_str().unwrap())
                        })
                        .unwrap();
                    if index % 2 == 1 {
                        bail!("HTTP 503")
                    }
                    Ok(json!({"stargazers_count":7,"pushed_at":"2020-01-01T00:00:00Z"}))
                });
                assert_eq!(
                    serde_json::to_value(result).unwrap(),
                    case["expected"],
                    "{}: {mode}",
                    capture["kind"]
                );
                let mut actual_requests = requests.into_inner().unwrap();
                let mut expected_requests: Vec<_> = case["requests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|url| url.as_str().unwrap().to_owned())
                    .collect();
                // Concurrent worker scheduling does not define request order.
                actual_requests.sort();
                expected_requests.sort();
                assert_eq!(actual_requests, expected_requests);
                if mode != "failed-topic" {
                    assert_eq!(serde_json::to_value(warnings).unwrap(), case["warnings"]);
                }
            }
        }
    }

    #[test]
    fn topic_match_and_parallel_fallback_preserve_order_and_missing_fields() {
        let calls = Mutex::new(Vec::new());
        let barrier = Barrier::new(2);
        let fetch = |path: &str, timeout| {
            calls.lock().unwrap().push((path.to_string(), timeout));
            if path.starts_with("/search/") {
                return Ok(json!({"items":[{"full_name":"OWNER/TAGGED","stargazers_count":0}]}));
            }
            barrier.wait(); // Both fallback requests must be in flight together.
            if path == "/repos/owner/missing" {
                Ok(json!({"pushed_at":"2020-01-01"}))
            } else {
                bail!("offline")
            }
        };
        let entries = vec![
            json!({"repo":"owner/missing","name":"first"}),
            json!({"repo":"Owner/Tagged"}),
            json!({"repo":"owner/offline"}),
        ];
        let (result, warnings) = resolve(&entries, "workdeck-extension", &fetch);
        assert_eq!(
            result,
            vec![
                json!({"repo":"owner/missing","name":"first","pushedAt":"2020-01-01"}),
                json!({"repo":"Owner/Tagged","stars":0}),
                entries[2].clone()
            ]
        );
        assert_eq!(
            warnings,
            [
                "Extension directory: owner/missing, owner/offline are listed but not tagged `workdeck-extension`."
            ]
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0],
            (
                "/search/repositories?q=topic%3Aworkdeck-extension%20is%3Apublic&per_page=100"
                    .into(),
                Duration::from_secs(8)
            )
        );
        assert!(
            calls[1..]
                .iter()
                .all(|(_, timeout)| *timeout == Duration::from_secs(5))
        );
    }

    #[test]
    fn failed_or_empty_topic_still_fetches_direct_without_untagged_warning() {
        for failed in [false, true] {
            let (result, warnings) = resolve(
                &[json!({"repo":"owner/repo"})],
                "workdeck-extension",
                &|path, _| {
                    if path.starts_with("/search/") {
                        if failed {
                            bail!("sensitive transport detail")
                        }
                        Ok(json!({"items":"malformed"}))
                    } else {
                        Ok(json!({"stargazers_count":7}))
                    }
                },
            );
            assert_eq!(result, [json!({"repo":"owner/repo","stars":7})]);
            assert_eq!(warnings.len(), usize::from(failed));
            assert!(
                warnings.iter().all(
                    |warning| !warning.contains("sensitive") && !warning.contains("not tagged")
                )
            );
        }
    }
}
