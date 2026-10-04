//! A small HTTP client over an injectable [`Transport`]: JSON decoding,
//! `Link`-header pagination, and retry with backoff on 429 and 5xx.

use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

/// A finished HTTP exchange, as the transport saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Response headers, names as sent.
    pub headers: Vec<(String, String)>,
    /// Body as text.
    pub body: String,
}

impl HttpResponse {
    /// A response with no headers.
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// The first header called `name`, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The `rel="next"` URL of the `Link` header, when present.
    pub fn next_link(&self) -> Option<String> {
        self.header("link").and_then(parse_next_link)
    }
}

/// Performs one GET. Implemented by [`UreqTransport`] and by scripted fakes
/// in tests. A `String` error is a failure to talk to the server at all.
pub trait Transport {
    /// GETs `url` with `headers`.
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String>;
}

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HttpError {
    /// The server could not be reached.
    #[error("{url}: {message}")]
    Transport {
        /// The URL.
        url: String,
        /// The transport's message.
        message: String,
    },
    /// 401 or 403.
    #[error("{url}: HTTP {status}, check the token")]
    Auth {
        /// The URL.
        url: String,
        /// 401 or 403.
        status: u16,
    },
    /// Any other non-success status, after retries for 429 and 5xx.
    #[error("{url}: HTTP {status} (starts with {excerpt:?})")]
    Status {
        /// The URL.
        url: String,
        /// The status.
        status: u16,
        /// The start of the body.
        excerpt: String,
    },
    /// The body is not the JSON we expected.
    #[error("{url}: {message} (starts with {excerpt:?})")]
    Decode {
        /// The URL.
        url: String,
        /// What is wrong.
        message: String,
        /// The start of the body.
        excerpt: String,
    },
}

/// How many attempts a request gets (first try plus retries).
pub const ATTEMPTS: u32 = 3;
/// Backoff base: 1s, 2s, 4s.
pub const BASE_DELAY: Duration = Duration::from_secs(1);
/// Pages followed at most by [`Client::get_all`].
pub const MAX_PAGES: usize = 20;
/// Characters of a body kept in error messages.
pub const EXCERPT_CHARS: usize = 200;

/// A client bound to one API: fixed headers (auth, accept) on every request.
pub struct Client {
    transport: Box<dyn Transport>,
    headers: Vec<(String, String)>,
    sleep: Box<dyn Fn(Duration)>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("Client")
            .field("headers", &names)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// A client over `transport` sending `headers` with every request;
    /// retries wait with `std::thread::sleep`.
    pub fn new(transport: Box<dyn Transport>, headers: Vec<(String, String)>) -> Self {
        Self {
            transport,
            headers,
            sleep: Box::new(std::thread::sleep),
        }
    }

    /// Replaces the function that waits between retries (tests record it).
    #[must_use]
    pub fn with_sleep(mut self, sleep: Box<dyn Fn(Duration)>) -> Self {
        self.sleep = sleep;
        self
    }

    /// GETs `url` and returns the response, retrying 429 and 5xx up to
    /// [`ATTEMPTS`] times with exponential backoff (or `Retry-After`).
    pub fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let mut attempt = 0;
        loop {
            let response =
                self.transport
                    .get(url, &self.headers)
                    .map_err(|message| HttpError::Transport {
                        url: url.to_owned(),
                        message,
                    })?;
            match response.status {
                200..=299 => return Ok(response),
                401 | 403 => {
                    return Err(HttpError::Auth {
                        url: url.to_owned(),
                        status: response.status,
                    });
                }
                429 | 500..=599 if attempt + 1 < ATTEMPTS => {
                    let delay = response
                        .header("retry-after")
                        .and_then(parse_retry_after)
                        .unwrap_or_else(|| backoff(attempt, BASE_DELAY));
                    (self.sleep)(delay);
                    attempt += 1;
                }
                status => {
                    return Err(HttpError::Status {
                        url: url.to_owned(),
                        status,
                        excerpt: excerpt(&response.body),
                    });
                }
            }
        }
    }

    /// GETs `url` and decodes the body as JSON.
    pub fn get_json(&self, url: &str) -> Result<Value, HttpError> {
        let response = self.get(url)?;
        decode(url, &response.body)
    }

    /// GETs `url` and every `Link: rel="next"` page after it (at most
    /// [`MAX_PAGES`]), concatenating the arrays.
    pub fn get_all(&self, url: &str) -> Result<Vec<Value>, HttpError> {
        let mut items = Vec::new();
        let mut next = Some(url.to_owned());
        let mut pages = 0;
        while let Some(page_url) = next {
            if pages == MAX_PAGES {
                break;
            }
            pages += 1;
            let response = self.get(&page_url)?;
            match decode(&page_url, &response.body)? {
                Value::Array(page) => items.extend(page),
                _ => {
                    return Err(HttpError::Decode {
                        url: page_url,
                        message: "expected a JSON array".to_owned(),
                        excerpt: excerpt(&response.body),
                    });
                }
            }
            next = response.next_link();
        }
        Ok(items)
    }
}

fn decode(url: &str, body: &str) -> Result<Value, HttpError> {
    serde_json::from_str(body).map_err(|e| HttpError::Decode {
        url: url.to_owned(),
        message: format!("invalid JSON: {e}"),
        excerpt: excerpt(body),
    })
}

/// `base * 2^attempt`.
pub fn backoff(attempt: u32, base: Duration) -> Duration {
    base.saturating_mul(2u32.saturating_pow(attempt))
}

/// A `Retry-After` value in seconds (dates are ignored).
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// The `rel="next"` target of a `Link` header value.
pub fn parse_next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let mut pieces = part.split(';');
        let target = pieces.next()?.trim();
        let is_next = pieces.any(|p| {
            let p = p.trim();
            p == "rel=\"next\"" || p == "rel=next"
        });
        if !is_next {
            return None;
        }
        let url = target.strip_prefix('<')?.strip_suffix('>')?;
        (!url.is_empty()).then(|| url.to_owned())
    })
}

/// The first [`EXCERPT_CHARS`] characters of `body`, trimmed, with control
/// characters replaced by spaces.
pub fn excerpt(body: &str) -> String {
    body.trim()
        .chars()
        .take(EXCERPT_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The real transport: `ureq` with rustls, a 30 second global timeout and
/// non-2xx statuses returned as responses rather than errors.
#[derive(Debug, Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    /// A transport with the default agent configuration.
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Transport for UreqTransport {
    /// Reason: network I/O; the client logic above is tested with scripted
    /// transports.
    #[mutants::skip]
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String> {
        let mut request = self.agent.get(url);
        for (k, v) in headers {
            request = request.header(k.as_str(), v.as_str());
        }
        let mut response = request.call().map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let response_headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_owned()))
            .collect();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok(HttpResponse {
            status,
            headers: response_headers,
            body,
        })
    }
}

/// A request as a [`ScriptedTransport`] recorded it: URL and headers.
pub type Recorded = (String, Vec<(String, String)>);

/// A transport answering from a script of responses, in order, recording
/// every request. For tests in this crate and its dependants.
#[derive(Debug, Default)]
pub struct ScriptedTransport {
    responses: std::cell::RefCell<std::collections::VecDeque<HttpResponse>>,
    requests: std::cell::RefCell<Vec<Recorded>>,
}

impl ScriptedTransport {
    /// A transport that will answer with `responses`, in order.
    pub fn new(responses: Vec<HttpResponse>) -> Self {
        Self {
            responses: std::cell::RefCell::new(responses.into()),
            requests: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// The URLs requested so far.
    pub fn urls(&self) -> Vec<String> {
        self.requests
            .borrow()
            .iter()
            .map(|(u, _)| u.clone())
            .collect()
    }

    /// The headers sent with request `index`.
    pub fn headers(&self, index: usize) -> Vec<(String, String)> {
        self.requests.borrow()[index].1.clone()
    }
}

impl Transport for ScriptedTransport {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String> {
        self.requests
            .borrow_mut()
            .push((url.to_owned(), headers.to_vec()));
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| format!("no scripted response for {url}"))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// Shares a scripted transport between the client and the test.
    struct Shared(Rc<ScriptedTransport>);

    impl Transport for Shared {
        fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpResponse, String> {
            self.0.get(url, headers)
        }
    }

    fn json(status: u16, body: &str) -> HttpResponse {
        HttpResponse::new(status, body)
    }

    fn make(
        responses: Vec<HttpResponse>,
    ) -> (Client, Rc<RefCell<Vec<Duration>>>, Rc<ScriptedTransport>) {
        let transport = Rc::new(ScriptedTransport::new(responses));
        let slept = Rc::new(RefCell::new(Vec::new()));
        let record = Rc::clone(&slept);
        let client = Client::new(
            Box::new(Shared(Rc::clone(&transport))),
            vec![("PRIVATE-TOKEN".to_owned(), "secret".to_owned())],
        )
        .with_sleep(Box::new(move |d| record.borrow_mut().push(d)));
        (client, slept, transport)
    }

    #[test]
    fn headers_are_sent_and_json_decoded() {
        let (client, slept, transport) = make(vec![json(200, "{\"id\": 7}")]);
        let value = client.get_json("https://api/user").unwrap();
        assert_eq!(value["id"], 7);
        assert_eq!(transport.urls(), vec!["https://api/user"]);
        assert_eq!(
            transport.headers(0),
            vec![("PRIVATE-TOKEN".to_owned(), "secret".to_owned())]
        );
        assert_eq!(slept.borrow().len(), 0);
    }

    #[test]
    fn retries_429_and_5xx_with_backoff_then_gives_up() {
        let (client, slept, transport) = make(vec![
            json(429, "slow down"),
            HttpResponse {
                status: 503,
                headers: vec![("Retry-After".to_owned(), "7".to_owned())],
                body: String::new(),
            },
            json(200, "[]"),
        ]);
        assert_eq!(
            client.get_json("https://api/x").unwrap(),
            Value::Array(Vec::new())
        );
        assert_eq!(transport.urls().len(), 3);
        assert_eq!(
            *slept.borrow(),
            vec![Duration::from_secs(1), Duration::from_secs(7)]
        );
        let (client, slept, _) = make(vec![json(500, "a"), json(502, "b"), json(503, "c\x01d")]);
        assert_eq!(
            client.get("https://api/x").unwrap_err(),
            HttpError::Status {
                url: "https://api/x".into(),
                status: 503,
                excerpt: "c d".into()
            }
        );
        assert_eq!(
            *slept.borrow(),
            vec![Duration::from_secs(1), Duration::from_secs(2)]
        );
    }

    #[test]
    fn auth_and_other_statuses_do_not_retry() {
        let (client, slept, _) = make(vec![json(401, "{}")]);
        assert_eq!(
            client.get("https://api/x").unwrap_err(),
            HttpError::Auth {
                url: "https://api/x".into(),
                status: 401
            }
        );
        let (client, _, _) = make(vec![json(403, "{}")]);
        assert!(matches!(
            client.get("https://api/x"),
            Err(HttpError::Auth { status: 403, .. })
        ));
        let (client, _, transport) = make(vec![json(404, "not found")]);
        assert_eq!(
            client.get_json("https://api/x").unwrap_err(),
            HttpError::Status {
                url: "https://api/x".into(),
                status: 404,
                excerpt: "not found".into()
            }
        );
        assert_eq!(transport.urls().len(), 1);
        assert_eq!(slept.borrow().len(), 0);
    }

    #[test]
    fn transport_and_decode_errors() {
        let (client, _, _) = make(vec![]);
        assert_eq!(
            client.get("https://api/x").unwrap_err(),
            HttpError::Transport {
                url: "https://api/x".into(),
                message: "no scripted response for https://api/x".into()
            }
        );
        let (client, _, _) = make(vec![json(200, "<html>")]);
        let err = client.get_json("https://api/x").unwrap_err();
        assert!(
            matches!(&err, HttpError::Decode { url, message, excerpt } if url == "https://api/x" && message.starts_with("invalid JSON") && excerpt == "<html>"),
            "{err}"
        );
        let (client, _, _) = make(vec![json(200, "{\"a\":1}")]);
        assert_eq!(
            client.get_all("https://api/x").unwrap_err(),
            HttpError::Decode {
                url: "https://api/x".into(),
                message: "expected a JSON array".into(),
                excerpt: "{\"a\":1}".into()
            }
        );
    }

    #[test]
    fn pagination_follows_link_next() {
        let page = |body: &str, next: Option<&str>| HttpResponse {
            status: 200,
            headers: next
                .map(|n| {
                    vec![(
                        "Link".to_owned(),
                        format!("<https://api/x?page=99>; rel=\"prev\", <{n}>; rel=\"next\""),
                    )]
                })
                .unwrap_or_default(),
            body: body.to_owned(),
        };
        let (client, _, transport) = make(vec![
            page("[1,2]", Some("https://api/x?page=2")),
            page("[3]", Some("https://api/x?page=3")),
            page("[]", None),
        ]);
        let items = client.get_all("https://api/x").unwrap();
        assert_eq!(items, vec![Value::from(1), Value::from(2), Value::from(3)]);
        assert_eq!(
            transport.urls(),
            vec![
                "https://api/x",
                "https://api/x?page=2",
                "https://api/x?page=3"
            ]
        );
    }

    #[test]
    fn pagination_stops_at_max_pages() {
        let mut responses = Vec::new();
        for i in 0..(MAX_PAGES + 5) {
            responses.push(HttpResponse {
                status: 200,
                headers: vec![(
                    "link".to_owned(),
                    format!("<https://api/x?page={}>; rel=next", i + 2),
                )],
                body: format!("[{i}]"),
            });
        }
        let (client, _, transport) = make(responses);
        let items = client.get_all("https://api/x").unwrap();
        assert_eq!(items.len(), MAX_PAGES);
        assert_eq!(transport.urls().len(), MAX_PAGES);
    }

    #[test]
    fn helpers() {
        assert_eq!(backoff(0, BASE_DELAY), Duration::from_secs(1));
        assert_eq!(backoff(1, BASE_DELAY), Duration::from_secs(2));
        assert_eq!(backoff(2, BASE_DELAY), Duration::from_secs(4));
        assert_eq!(parse_retry_after(" 12 "), Some(Duration::from_secs(12)));
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
        assert_eq!(
            parse_next_link("<https://a?page=2>; rel=\"next\", <https://a?page=9>; rel=\"last\""),
            Some("https://a?page=2".to_owned())
        );
        assert_eq!(parse_next_link("<https://a?page=9>; rel=\"last\""), None);
        assert_eq!(parse_next_link("<>; rel=\"next\""), None);
        assert_eq!(parse_next_link("https://a; rel=\"next\""), None);
        assert_eq!(excerpt("  hi\nthere  "), "hi there");
        let long = "x".repeat(500);
        assert_eq!(excerpt(&long).chars().count(), EXCERPT_CHARS);
        let r = HttpResponse {
            status: 200,
            headers: vec![("Content-Type".to_owned(), "a".to_owned())],
            body: String::new(),
        };
        assert_eq!(r.header("content-type"), Some("a"));
        assert_eq!(r.header("x"), None);
        assert_eq!(r.next_link(), None);
        assert_eq!(
            format!(
                "{:?}",
                Client::new(
                    Box::new(ScriptedTransport::default()),
                    vec![("A".into(), "b".into())]
                )
            ),
            "Client { headers: [\"A\"], .. }"
        );
        let _ = UreqTransport::default();
    }

    #[test]
    fn error_messages() {
        assert_eq!(
            HttpError::Transport {
                url: "u".into(),
                message: "m".into()
            }
            .to_string(),
            "u: m"
        );
        assert_eq!(
            HttpError::Auth {
                url: "u".into(),
                status: 401
            }
            .to_string(),
            "u: HTTP 401, check the token"
        );
        assert_eq!(
            HttpError::Status {
                url: "u".into(),
                status: 500,
                excerpt: "e".into()
            }
            .to_string(),
            "u: HTTP 500 (starts with \"e\")"
        );
        assert_eq!(
            HttpError::Decode {
                url: "u".into(),
                message: "m".into(),
                excerpt: "e".into()
            }
            .to_string(),
            "u: m (starts with \"e\")"
        );
    }
}
