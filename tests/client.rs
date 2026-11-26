use std::sync::{Arc, Mutex};

use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use trustcomponent::apis::captcha_verifications_api::GetResultParams;
use trustcomponent::apis::platform_events_api::GetEventsParams;
use trustcomponent::apis::tax_validation_validation_results_api::GetValidationResultParams;
use trustcomponent::{Error, TrustComponent};

const ID: &str = "cc2e2d5e-d1ef-4a7f-a7bd-dec5b37df47a";
const TS: &str = "2020-01-01T13:30:05.941Z";

fn result() -> String {
    serde_json::json!({"captchaId": ID, "verificationId": ID, "verificationPassed": true, "score": 0.3, "decisionType": "STANDARD", "decisionAction": "ALLOW",
        "gatewayFailoverActive": false, "riskScoringEnabled": true, "minimalDataModeEnabled": false, "origin": "https://example.com", "ipAddress": "1.2.3.4",
        "countryCode": "DE", "deviceFamily": "Desktop", "operatingSystem": "Linux", "browser": "Firefox", "verificationStartedAt": TS, "verificationFinishedAt": TS,
        "resultExpiresAt": TS, "resultFirstFetchedAt": TS, "resultLastFetchedAt": TS}).to_string()
}

#[derive(Clone)]
struct Answer { status: u16, headers: Vec<(&'static str, &'static str)>, body: String }

fn ok(headers: Vec<(&'static str, &'static str)>) -> Answer { Answer { status: 200, headers, body: result() } }
fn status(status: u16, headers: Vec<(&'static str, &'static str)>, body: &str) -> Answer { Answer { status, headers, body: body.to_owned() } }

struct Fake { requests: Arc<Mutex<Vec<String>>>, warnings: Arc<Mutex<Vec<String>>> }

async fn start(answers: Vec<Answer>) -> (TrustComponent, Fake) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = requests.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 16384];
            let n = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let answer = { let mut r = seen.lock().unwrap(); r.push(request); answers[(r.len() - 1).min(answers.len() - 1)].clone() };
            let mut response = format!("HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", answer.status, answer.body.len());
            for (k, v) in &answer.headers { response += &format!("{k}: {v}\r\n"); }
            response += "\r\n";
            response += &answer.body;
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        }
    });
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let sink = warnings.clone();
    let tc = TrustComponent::builder("ak_test").host(&format!("http://{address}")).integration("test-suite/1.0")
        .logger(move |m| sink.lock().unwrap().push(m.to_owned())).build().unwrap();
    (tc, Fake { requests, warnings })
}

fn token(client_failover: bool) -> String {
    base64::engine::general_purpose::STANDARD.encode(serde_json::json!({"verificationId": ID, "clientFailover": client_failover}).to_string())
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|l| l.split_once(": ").filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v))
}

fn params() -> GetResultParams { GetResultParams { verification_id: ID.parse().unwrap(), client_failover: None } }

#[tokio::test]
async fn sends_key_user_agent_and_metadata_and_decodes_the_widget_token() {
    let (tc, fake) = start(vec![ok(vec![])]).await;
    let result = tc.captcha.get_verification_result(&token(true)).await.unwrap();
    assert_eq!(result.verification_passed, Some(true));
    let request = fake.requests.lock().unwrap()[0].clone();
    assert!(request.starts_with(&format!("GET /captcha/v2/verifications/{ID}/results?clientFailover=true ")));
    assert_eq!(header(&request, "authorization"), Some("Bearer ak_test"));
    assert!(header(&request, "user-agent").unwrap().starts_with("trustcomponent-rust/"));
    let metadata: serde_json::Value = serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(header(&request, "trustcomponent-sdk").unwrap()).unwrap()).unwrap();
    assert_eq!(metadata["sdk"], "rust");
    assert_eq!(metadata["integration"], "test-suite/1.0");
}

#[tokio::test]
async fn an_invalid_widget_token_fails_before_any_request() {
    let (tc, fake) = start(vec![ok(vec![])]).await;
    assert!(matches!(tc.captcha.get_verification_result("not-a-token").await, Err(Error::VerificationTokenInvalid)));
    assert!(fake.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn retries_503_and_429_with_retry_after_then_succeeds() {
    let (tc, fake) = start(vec![status(503, vec![], ""), status(429, vec![("Retry-After", "0")], ""), ok(vec![])]).await;
    tc.captcha.verifications.get_result(params()).await.unwrap();
    assert_eq!(fake.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_429_without_retry_after_is_final_and_not_retried() {
    let (tc, fake) = start(vec![status(429, vec![], ""), ok(vec![])]).await;
    let error = tc.captcha.verifications.get_result(params()).await.unwrap_err();
    assert!(error.api().unwrap().is_rate_limit());
    assert_eq!(fake.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn problem_details_become_typed_errors_with_the_code_of_the_api() {
    let (tc, _) = start(vec![status(404, vec![], r#"{"status":404,"code":"validation_result_not_found","detail":"detail"}"#)]).await;
    let error = tc.tax_validation.validation_results.get_validation_result(GetValidationResultParams { validation_result_id: ID.parse().unwrap() }).await.unwrap_err();
    let api = error.api().unwrap();
    assert!(api.is_not_found());
    assert_eq!(api.code.as_deref(), Some("validation_result_not_found"));
}

#[tokio::test]
async fn a_client_failover_rejected_by_the_gateway_is_a_conflict() {
    let (tc, _) = start(vec![status(412, vec![], r#"{"reason":"CLIENT_FAILOVER_REJECTED"}"#)]).await;
    assert!(tc.captcha.get_verification_result(&token(true)).await.unwrap_err().api().unwrap().is_conflict());
}

#[tokio::test]
async fn a_deprecated_operation_warns_once_a_retired_one_asks_for_an_update() {
    let deprecated = vec![("Deprecation", "@1799971200"), ("Sunset", "Thu, 15 Jul 2027 00:00:00 GMT")];
    let (tc, fake) = start(vec![ok(deprecated.clone()), ok(deprecated), status(410, vec![], r#"{"status":410,"code":"operation_retired","detail":"retired"}"#)]).await;
    for _ in 0..2 { tc.captcha.verifications.get_result(params()).await.unwrap(); }
    let warnings = fake.warnings.lock().unwrap().clone();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("15 Jul 2027"));
    let error = tc.platform.events.get_events(GetEventsParams { from: None, to: None, cursor: None, page: None, page_size: None }).await.unwrap_err();
    assert!(error.api().unwrap().is_operation_retired());
    assert!(error.to_string().contains("Update the TrustComponent SDK"));
}
