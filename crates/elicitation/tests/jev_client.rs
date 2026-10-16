//! Wire-level tests for the Jev HTTP client. A local stub HTTP server records
//! requests and replies with canned responses; no real Jev/OpenRouter call is
//! ever made.

#![cfg(feature = "jev")]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use elicitation::assist::UnavailableReason;
use elicitation::assist::jev::{
    HttpJudge, JevAnswer, JevConfig, JevQuestion, Judge, MAX_QUESTIONS_PER_CALL,
};
use serde_json::{Value, json};

/// A minimal single-purpose HTTP stub. It answers every accepted connection
/// with the configured status/body and records the request head and body.
struct Stub {
    addr: SocketAddr,
    hits: Arc<AtomicUsize>,
    heads: Arc<Mutex<Vec<String>>>,
    bodies: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    _handle: JoinHandle<()>,
}

impl Stub {
    fn start(status: u16, body: impl Into<String>, delay: Option<Duration>) -> Self {
        let body = body.into();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = listener.local_addr().expect("local addr");
        let hits = Arc::new(AtomicUsize::new(0));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let hits = Arc::clone(&hits);
            let heads = Arc::clone(&heads);
            let bodies = Arc::clone(&bodies);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            hits.fetch_add(1, Ordering::SeqCst);
                            if let Some((head, request_body)) = read_request(&mut stream) {
                                heads.lock().expect("heads").push(head);
                                bodies.lock().expect("bodies").push(request_body);
                            }
                            if let Some(delay) = delay {
                                thread::sleep(delay);
                            }
                            let response = format!(
                                "HTTP/1.1 {status} Stub\r\nContent-Type: application/json\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.flush();
                        }
                        Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            addr,
            hits,
            heads,
            bodies,
            stop,
            _handle: handle,
        }
    }

    fn uri(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    fn body(&self, index: usize) -> Value {
        serde_json::from_str(&self.bodies.lock().expect("bodies")[index]).expect("json body")
    }

    fn head(&self, index: usize) -> String {
        self.heads.lock().expect("heads")[index].clone()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Reads one HTTP request, returning `(head, body)`.
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut head = String::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        head.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    Some((head, String::from_utf8_lossy(&body).to_string()))
}

fn config_for(uri: &str, timeout_secs: u64) -> JevConfig {
    let timeout = timeout_secs.to_string();
    JevConfig::from_lookup(&|key| match key {
        "ELICITATION_ASSIST" => Some("jev".to_string()),
        "TYPESAFE_JEV_API_KEY" => Some("sk-test".to_string()),
        "ELICITATION_JEV_BASE_URL" => Some(uri.to_string()),
        "ELICITATION_JEV_TIMEOUT_SECS" => Some(timeout.clone()),
        _ => None,
    })
    .expect("test config must build")
}

fn noul_question() -> JevQuestion {
    JevQuestion::Noul {
        instructions: "is it urgent?".to_string(),
        criteria: None,
    }
}

fn one_noul_question() -> BTreeMap<String, JevQuestion> {
    BTreeMap::from([("q".to_string(), noul_question())])
}

fn noul_questions(count: usize) -> BTreeMap<String, JevQuestion> {
    (0..count)
        .map(|index| (format!("q{index}"), noul_question()))
        .collect()
}

fn noul_reply() -> String {
    json!({
        "model": "jev-latest",
        "answers": {"q": {"type": "noul", "noul": 0.25}},
        "usage": {"input_tokens": 1, "output_tokens": 0}
    })
    .to_string()
}

#[tokio::test]
async fn typesafe_choice_reply_decodes_answer() {
    let stub = Stub::start(
        200,
        json!({
            "model": "jev-latest",
            "answers": {
                "q": {
                    "type": "choice",
                    "choice": "aida",
                    "probabilities": {"aida": 0.6, "pas": 0.4},
                    "confidence": 0.55
                }
            }
        })
        .to_string(),
        None,
    );
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));
    let questions = BTreeMap::from([(
        "q".to_string(),
        JevQuestion::Choice {
            instructions: "family?".to_string(),
            criteria: BTreeMap::from([
                ("aida".to_string(), "d".to_string()),
                ("pas".to_string(), "d".to_string()),
            ]),
        },
    )]);

    let reply = judge
        .ask(json!({"text": "hi"}), questions)
        .await
        .expect("choice reply must decode");

    assert_eq!(
        reply.answers["q"],
        JevAnswer::Choice {
            choice: "aida".to_string(),
            probabilities: BTreeMap::from([("aida".to_string(), 0.6), ("pas".to_string(), 0.4),]),
            confidence: 0.55,
        }
    );
}

#[tokio::test]
async fn request_carries_bearer_authorization() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let _ = judge.ask(json!({"text": "hi"}), one_noul_question()).await;
    let head = stub.head(0).to_ascii_lowercase();

    assert!(head.contains("authorization: bearer sk-test"));
}

#[tokio::test]
async fn request_body_carries_model() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let _ = judge.ask(json!({"text": "hi"}), one_noul_question()).await;
    let body = stub.body(0);

    assert_eq!(body["model"], "jev-latest");
}

#[tokio::test]
async fn request_body_carries_questions() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let _ = judge.ask(json!({"text": "hi"}), one_noul_question()).await;
    let body = stub.body(0);

    assert_eq!(body["questions"]["q"]["type"], "noul");
}

#[tokio::test]
async fn request_body_carries_state() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let _ = judge.ask(json!({"text": "hi"}), one_noul_question()).await;
    let body = stub.body(0);

    assert_eq!(body["state"], json!({"text": "hi"}));
}

#[tokio::test]
async fn numeric_usage_cost_decodes_to_cost_usd() {
    let stub = Stub::start(
        200,
        json!({
            "model": "jev-latest",
            "answers": {"q": {"type": "noul", "noul": 0.25}},
            "usage": {"input_tokens": 3, "output_tokens": 0, "cost": 0.25}
        })
        .to_string(),
        None,
    );
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let reply = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect("reply must decode");

    assert_eq!(reply.usage.cost_usd, Some(0.25));
}

#[tokio::test]
async fn object_usage_cost_decodes_to_cost_usd() {
    let stub = Stub::start(
        200,
        json!({
            "model": "jev-latest",
            "answers": {"q": {"type": "noul", "noul": 0.25}},
            "usage": {"input_tokens": 3, "output_tokens": 0, "cost": {"total": 0.5}}
        })
        .to_string(),
        None,
    );
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let reply = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect("reply must decode");

    assert_eq!(reply.usage.cost_usd, Some(0.5));
}

#[tokio::test]
async fn http_401_yields_http4xx() {
    let stub = Stub::start(401, "", None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("401 must fail");

    assert_eq!(err.reason, UnavailableReason::Http4xx);
}

#[tokio::test]
async fn http_401_detail_scrubs_api_key() {
    let stub = Stub::start(401, "invalid key sk-test", None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("401 must fail");

    assert!(!err.detail.contains("sk-test"));
}

#[tokio::test]
async fn http_429_yields_http4xx() {
    let stub = Stub::start(429, "", None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("429 must fail");

    assert_eq!(err.reason, UnavailableReason::Http4xx);
}

#[tokio::test]
async fn http_503_yields_http5xx() {
    let stub = Stub::start(503, "", None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("503 must fail");

    assert_eq!(err.reason, UnavailableReason::Http5xx);
}

#[tokio::test]
async fn invalid_json_yields_bad_response() {
    let stub = Stub::start(200, "{not json", None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("invalid json must fail");

    assert_eq!(err.reason, UnavailableReason::BadResponse);
}

#[tokio::test]
async fn missing_answer_id_yields_bad_response() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));
    let mut questions = one_noul_question();
    questions.insert("q2".to_string(), noul_question());

    let err = judge
        .ask(json!({"text": "hi"}), questions)
        .await
        .expect_err("missing answer id must fail");

    assert_eq!(err.reason, UnavailableReason::BadResponse);
}

#[tokio::test]
async fn delayed_reply_yields_timeout() {
    let stub = Stub::start(200, noul_reply(), Some(Duration::from_secs(5)));
    let judge = HttpJudge::new(config_for(&stub.uri(), 1));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("delay must time out");

    assert_eq!(err.reason, UnavailableReason::Timeout);
}

#[tokio::test]
async fn refused_connection_yields_network() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let port = listener.local_addr().expect("ephemeral addr").port();
    drop(listener);
    let judge = HttpJudge::new(config_for(&format!("http://127.0.0.1:{port}"), 5));

    let err = judge
        .ask(json!({"text": "hi"}), one_noul_question())
        .await
        .expect_err("refused connection must fail");

    assert_eq!(err.reason, UnavailableReason::Network);
}

#[tokio::test]
async fn fifty_one_questions_yields_bad_config() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let err = judge
        .ask(
            json!({"text": "hi"}),
            noul_questions(MAX_QUESTIONS_PER_CALL + 1),
        )
        .await
        .expect_err("too many questions must fail");

    assert_eq!(err.reason, UnavailableReason::BadConfig);
}

#[tokio::test]
async fn fifty_one_questions_sends_no_request() {
    let stub = Stub::start(200, noul_reply(), None);
    let judge = HttpJudge::new(config_for(&stub.uri(), 5));

    let _ = judge
        .ask(
            json!({"text": "hi"}),
            noul_questions(MAX_QUESTIONS_PER_CALL + 1),
        )
        .await;

    assert!(stub.hits() == 0);
}
