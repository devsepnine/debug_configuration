//! HTTP 전송 계층 — Streamable HTTP의 단일 엔드포인트 `POST /mcp`.
//!
//! 루프백에만 바인드하고, Origin 검증 → Bearer 인증 → 본문 상한을 통과한 요청만
//! JSON-RPC로 넘긴다. SSE 스트림과 resumability는 제공하지 않으므로 `GET`/`DELETE`는 `405`다.

use super::protocol::{
    INTERNAL_ERROR, INVALID_PARAMS, JsonRpcError, METHOD_NOT_FOUND, Request, VersionCheck,
    check_protocol_version, discover_result, failure, initialize_result, parse_request, success,
    tool_error, tool_result, unsupported_version_error,
};
use super::{McpEvent, McpOp, McpOutcome, McpRequest, McpServerConfig, tools};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{HeaderMap, Method, Response, StatusCode, header};
use hyper_util::rt::{TokioIo, TokioTimer};
use iced::futures::SinkExt;
use iced::futures::channel::mpsc as iced_mpsc;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// 유일한 엔드포인트 경로.
const ENDPOINT_PATH: &str = "/mcp";
/// 요청 본문 상한. 우리 툴의 인자는 모두 작고, 상한이 없으면 로컬 클라이언트 하나가
/// 앱 메모리를 채울 수 있다.
const MAX_BODY_BYTES: usize = 1024 * 1024;
/// update 루프 응답 대기 상한. GUI가 모달·긴 프레임으로 막혀 있어도 클라이언트는
/// 무한 대기하지 않고 JSON-RPC 에러를 받는다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 요청 헤더 수신 상한. 인증은 헤더를 다 읽은 뒤에 판정하므로, 이 상한이 없으면
/// 토큰 없는 연결이 헤더 일부만 흘리며 소켓과 태스크를 무기한 점유할 수 있다.
/// hyper는 타이머가 설정되지 않으면 이 값을 조용히 무시한다(`common/time.rs`의 `check`).
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// modern 리비전이 협상된 버전을 싣는 요청 헤더.
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// 연결 핸들러들이 공유하는 상태.
struct ServerContext {
    /// 기대 Bearer 토큰.
    token: String,
    /// update 루프로 요청을 올려보내는 채널.
    output: iced_mpsc::Sender<McpEvent>,
}

/// 리스너를 열고 연결을 처리한다. 반환하면(바인드 실패) 스트림이 끝난다.
pub(super) async fn serve(config: McpServerConfig, mut output: iced_mpsc::Sender<McpEvent>) {
    // 루프백 전용. 이 코드에 `0.0.0.0` 경로를 두지 않는 것이 외부 노출을 막는 1차 방어선이다.
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, config.port)).await {
        Ok(listener) => listener,
        Err(e) => {
            let _ = output
                .send(McpEvent::BindFailed(format!(
                    "127.0.0.1:{}: {e}",
                    config.port
                )))
                .await;
            return;
        }
    };

    let port = listener
        .local_addr()
        .map_or(config.port, |addr| addr.port());
    if output.send(McpEvent::Started { port }).await.is_err() {
        return;
    }

    let ctx = Arc::new(ServerContext {
        token: config.token,
        output,
    });

    // JoinSet: 이 future가 drop되면(설정 변경으로 subscription 재시작) 진행 중인 연결까지
    // abort된다. 낡은 토큰을 들고 있는 핸들러가 살아남지 않는다.
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _peer)) => {
                    let ctx = Arc::clone(&ctx);
                    connections.spawn(async move {
                        let service = service_fn(move |req| handle(req, Arc::clone(&ctx)));
                        if let Err(e) = http1::Builder::new()
                            .timer(TokioTimer::new())
                            .header_read_timeout(HEADER_READ_TIMEOUT)
                            .serve_connection(TokioIo::new(stream), service)
                            .await
                            // 헤더 타임아웃은 유휴 keep-alive 연결에서도 정상적으로 발생한다
                            // — 에러로 찍으면 진단 로그가 그 소음에 묻힌다.
                            && !e.is_timeout()
                        {
                            // 클라이언트 조기 종료는 흔하다 — 진단용으로만 남긴다.
                            eprintln!("[MCP] Connection closed with error: {e}");
                        }
                    });
                }
                Err(e) => {
                    // accept 실패로 루프를 끝내면 사용자가 설정을 토글해야 복구된다.
                    // fd 고갈 같은 지속적 실패에서 바쁜 루프가 되지 않도록 잠깐 쉰다.
                    eprintln!("[MCP] Accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            },
            Some(_) = connections.join_next() => {}
        }
    }
}

async fn handle(
    req: hyper::Request<Incoming>,
    ctx: Arc<ServerContext>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(route(req, &ctx).await)
}

async fn route(req: hyper::Request<Incoming>, ctx: &ServerContext) -> Response<Full<Bytes>> {
    let (parts, body) = req.into_parts();

    // DNS rebinding 방어: 브라우저가 붙인 Origin이 루프백이 아니면 거부한다.
    // 헤더 부재는 비브라우저 클라이언트(Claude Code)의 정상 요청이다.
    if !origin_allowed(header_str(&parts.headers, header::ORIGIN.as_str())) {
        return text_response(StatusCode::FORBIDDEN, "Origin not allowed");
    }
    if !authorized(&ctx.token, &parts.headers) {
        return unauthorized();
    }
    if parts.method != Method::POST {
        // SSE·resumability를 제공하지 않으므로 GET/DELETE는 스펙이 지시하는 405다.
        return method_not_allowed();
    }
    if parts.uri.path() != ENDPOINT_PATH {
        return text_response(StatusCode::NOT_FOUND, "Not found");
    }
    // 상한 판정은 `Limited` 하나로 한다. `Content-Length`만 보고 본문을 읽기 전에 끊으면
    // 클라이언트가 아직 쓰는 중인 소켓을 닫아, 413 대신 connection reset을 보게 된다.
    let body = match Limited::new(body, MAX_BODY_BYTES).collect().await {
        Ok(collected) => collected.to_bytes(),
        // Limited의 에러는 상한 초과(`LengthLimitError`)이거나 전송 중단이다. downcast로
        // 구분할 수는 있지만 전송이 끊긴 클라이언트는 어떤 응답도 읽지 않으므로, 관측되는
        // 유일한 경우인 상한 초과 하나로 응답한다.
        Err(_) => return payload_too_large(),
    };

    let request = match parse_request(&body) {
        Ok(request) => request,
        Err(e) => return json_response(&failure(None, &e)),
    };

    // notification은 응답 본문을 받지 않는다(스펙: 202 Accepted). 버전 검사보다 앞에 두어야
    // 한다 — 뒤에 두면 버전 오류가 id 없는 요청에 `"id": null` 에러 응답을 돌려준다.
    if request.is_notification() {
        return empty_response(StatusCode::ACCEPTED);
    }

    let header_version = header_str(&parts.headers, PROTOCOL_VERSION_HEADER);
    match check_protocol_version(header_version, &request.params) {
        VersionCheck::Ok(_) => {}
        VersionCheck::Mismatch { header, body } => {
            return text_response(
                StatusCode::BAD_REQUEST,
                &format!(
                    "MCP-Protocol-Version header ({header}) does not match the request body ({body})"
                ),
            );
        }
        VersionCheck::Unsupported(requested) => {
            return json_response(&failure(
                request.id.clone(),
                &unsupported_version_error(&requested),
            ));
        }
    }

    let id = request.id.clone();
    match dispatch(&request, ctx).await {
        Ok(result) => json_response(&success(id, result)),
        Err(e) => json_response(&failure(id, &e)),
    }
}

/// JSON-RPC 메서드 라우팅.
async fn dispatch(request: &Request, ctx: &ServerContext) -> Result<Value, JsonRpcError> {
    match request.method.as_str() {
        "server/discover" => Ok(discover_result(crate::services::CURRENT_VERSION)),
        "initialize" => Ok(initialize_result(
            request
                .params
                .get("protocolVersion")
                .and_then(Value::as_str),
            crate::services::CURRENT_VERSION,
        )),
        "ping" => Ok(json!({})),
        "tools/list" => match bridge(ctx, McpOp::ListTools).await? {
            McpOutcome::ToolList(tools) => Ok(json!({ "tools": tools })),
            other => Err(unexpected_outcome("tools/list", &other)),
        },
        "tools/call" => {
            let name = request
                .tool_name()
                .ok_or_else(|| JsonRpcError::new(INVALID_PARAMS, "Missing tool name"))?;
            let op = tools::parse_call(name, &request.arguments())?;
            match bridge(ctx, op).await? {
                McpOutcome::Payload(payload) => Ok(tool_result(&payload)),
                McpOutcome::Failure(message) => Ok(tool_error(&message)),
                // 목록에 없는 툴을 호출한 경우와 같은 코드로 답한다 — 허용되지 않은 툴은
                // 클라이언트 입장에서 존재하지 않는 툴이다.
                McpOutcome::Denied(message) => Err(JsonRpcError::new(METHOD_NOT_FOUND, message)),
                other => Err(unexpected_outcome("tools/call", &other)),
            }
        }
        other => Err(JsonRpcError::new(
            METHOD_NOT_FOUND,
            format!("Method not found: {other}"),
        )),
    }
}

/// 요청을 update 루프로 보내고 응답을 기다린다.
async fn bridge(ctx: &ServerContext, op: McpOp) -> Result<McpOutcome, JsonRpcError> {
    let (reply, mut answers) = mpsc::channel(1);
    let mut output = ctx.output.clone();

    if output
        .send(McpEvent::Request(McpRequest { op, reply }))
        .await
        .is_err()
    {
        return Err(JsonRpcError::new(
            INTERNAL_ERROR,
            "The app is no longer accepting MCP requests",
        ));
    }

    match tokio::time::timeout(REQUEST_TIMEOUT, answers.recv()).await {
        Ok(Some(outcome)) => Ok(outcome),
        Ok(None) => Err(JsonRpcError::new(
            INTERNAL_ERROR,
            "The app dropped the request without responding",
        )),
        Err(_) => Err(JsonRpcError::new(
            INTERNAL_ERROR,
            format!(
                "The app did not respond within {}s",
                REQUEST_TIMEOUT.as_secs()
            ),
        )),
    }
}

/// 앱이 메서드와 짝이 맞지 않는 결과를 돌려준 경우 — 배선 실수다.
fn unexpected_outcome(method: &str, outcome: &McpOutcome) -> JsonRpcError {
    let kind = match outcome {
        McpOutcome::ToolList(_) => "tool list",
        McpOutcome::Payload(_) => "payload",
        McpOutcome::Failure(_) => "failure",
        McpOutcome::Denied(_) => "denial",
    };
    JsonRpcError::new(
        INTERNAL_ERROR,
        format!("{method} received an unexpected {kind} response"),
    )
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// 허용되는 루프백 호스트. IPv6 리터럴은 Origin 헤더에 나타나는 형태(대괄호 포함)로 둔다.
const LOOPBACK_ORIGIN_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

/// Origin 허용 여부. 부재는 허용(비브라우저 클라이언트), 그 외에는 루프백 출처만 허용한다.
///
/// 호스트는 **정확히** 일치해야 하고 뒤에 붙을 수 있는 것은 `:<포트>`뿐이다 —
/// `http://localhost.evil.com`처럼 접두사만 같은 출처가 통과하면 DNS rebinding 방어가 무너진다.
fn origin_allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    // 브라우저가 프라이버시 목적으로 보내는 불투명 출처(`null`)와 http 이외 스킴은
    // 어떤 사이트인지 알 수 없으므로 거부한다.
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    LOOPBACK_ORIGIN_HOSTS
        .iter()
        .any(|host| match authority.strip_prefix(host) {
            Some("") => true,
            Some(port) => port.strip_prefix(':').is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
            }),
            None => false,
        })
}

/// Bearer 토큰 검증.
fn authorized(expected: &str, headers: &HeaderMap) -> bool {
    let Some(value) = header_str(headers, header::AUTHORIZATION.as_str()) else {
        return false;
    };
    // 스킴은 대소문자 구분이 없다(RFC 7235).
    let Some((scheme, provided)) = value.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    super::token::matches(expected, provided.trim())
}

fn json_response(value: &Value) -> Response<Full<Bytes>> {
    let body = serde_json::to_vec(value).unwrap_or_else(|e| {
        format!("{{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{{\"code\":{INTERNAL_ERROR},\"message\":\"serialization failed: {e}\"}}}}")
            .into_bytes()
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .expect("static response parts are valid")
}

fn text_response(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(message.to_string())))
        .expect("static response parts are valid")
}

fn empty_response(status: StatusCode) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .body(Full::new(Bytes::new()))
        .expect("static response parts are valid")
}

fn unauthorized() -> Response<Full<Bytes>> {
    let mut response = text_response(StatusCode::UNAUTHORIZED, "Missing or invalid bearer token");
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        header::HeaderValue::from_static("Bearer"),
    );
    response
}

fn method_not_allowed() -> Response<Full<Bytes>> {
    let mut response = text_response(
        StatusCode::METHOD_NOT_ALLOWED,
        "Only POST is supported; this server does not offer SSE streams",
    );
    response
        .headers_mut()
        .insert(header::ALLOW, header::HeaderValue::from_static("POST"));
    response
}

fn payload_too_large() -> Response<Full<Bytes>> {
    text_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "Request body exceeds 1 MiB limit",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::mcp::protocol::{PROTOCOL_LEGACY, PROTOCOL_MODERN};
    use iced::futures::StreamExt;
    use std::collections::HashMap;

    /// 포트 0으로 뜬 테스트 서버 + 가짜 update 루프.
    ///
    /// 실제 앱 상태를 띄우지 않고, 미리 정한 결과를 돌려주는 핸들러로 HTTP 계층만 검증한다.
    struct TestServer {
        base_url: String,
        token: String,
        server: tokio::task::JoinHandle<()>,
        app: tokio::task::JoinHandle<()>,
    }

    impl TestServer {
        /// `answers`: 툴 이름 또는 `"tools/list"` → 돌려줄 결과.
        async fn start(answers: HashMap<&'static str, McpOutcome>) -> Self {
            let (base_url, token, server, mut events) = Self::bind().await;

            let app = tokio::spawn(async move {
                while let Some(event) = events.next().await {
                    let McpEvent::Request(request) = event else {
                        continue;
                    };
                    let key = match request.op() {
                        McpOp::ListTools => "tools/list",
                        McpOp::ListSessions => "list_sessions",
                        McpOp::ListConfigurations => "list_configurations",
                        _ => "other",
                    };
                    let outcome = answers
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| McpOutcome::Failure(format!("no answer for {key}")));
                    request.respond(outcome);
                }
            });

            Self {
                base_url,
                token,
                server,
                app,
            }
        }

        /// 요청을 받고 응답 없이 버리는 앱. 브리지가 무응답을 에러로 바꾸는지 검증한다.
        async fn start_dropping_requests() -> Self {
            let (base_url, token, server, mut events) = Self::bind().await;
            let app = tokio::spawn(async move { while events.next().await.is_some() {} });

            Self {
                base_url,
                token,
                server,
                app,
            }
        }

        /// 포트 0으로 리스너를 띄우고 `Started`까지 확인한다.
        async fn bind() -> (
            String,
            String,
            tokio::task::JoinHandle<()>,
            iced_mpsc::Receiver<McpEvent>,
        ) {
            let token = "a".repeat(64);
            let (output, mut events) = iced_mpsc::channel(16);
            let config = McpServerConfig {
                port: 0,
                token: token.clone(),
            };

            let server = tokio::spawn(serve(config, output));

            let port = match events.next().await {
                Some(McpEvent::Started { port }) => port,
                other => panic!("expected Started, got {other:?}"),
            };

            (format!("http://127.0.0.1:{port}"), token, server, events)
        }

        fn url(&self) -> String {
            format!("{}{ENDPOINT_PATH}", self.base_url)
        }

        fn client() -> reqwest::Client {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("client")
        }

        /// 인증된 JSON-RPC 요청.
        async fn call(&self, body: Value) -> (StatusCode, Value) {
            let response = Self::client()
                .post(self.url())
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .expect("request");
            let status = StatusCode::from_u16(response.status().as_u16()).expect("status");
            let text = response.text().await.expect("body");
            let value = serde_json::from_str(&text).unwrap_or(Value::Null);
            (status, value)
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.server.abort();
            self.app.abort();
        }
    }

    fn request(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[tokio::test]
    async fn discover_answers_without_reaching_the_app() {
        let server = TestServer::start(HashMap::new()).await;
        let (status, body) = server.call(request("server/discover", json!({}))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["supportedVersions"][0], PROTOCOL_MODERN);
        assert_eq!(body["result"]["resultType"], "complete");
    }

    #[tokio::test]
    async fn initialize_echoes_the_requested_version() {
        let server = TestServer::start(HashMap::new()).await;
        let (status, body) = server
            .call(request(
                "initialize",
                json!({ "protocolVersion": PROTOCOL_LEGACY }),
            ))
            .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["protocolVersion"], PROTOCOL_LEGACY);
    }

    #[tokio::test]
    async fn missing_token_is_rejected() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .json(&request("server/discover", json!({})))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 401);
        assert_eq!(
            response
                .headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok()),
            Some("Bearer")
        );
    }

    #[tokio::test]
    async fn wrong_token_is_rejected() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth("b".repeat(64))
            .json(&request("server/discover", json!({})))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 401);
    }

    #[tokio::test]
    async fn non_loopback_origin_is_rejected() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("origin", "http://evil.example.com")
            .json(&request("server/discover", json!({})))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 403);
    }

    #[tokio::test]
    async fn loopback_origin_is_accepted() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("origin", "http://localhost:5173")
            .json(&request("server/discover", json!({})))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn get_and_delete_are_not_allowed() {
        let server = TestServer::start(HashMap::new()).await;

        for builder in [
            TestServer::client().get(server.url()),
            TestServer::client().delete(server.url()),
        ] {
            let response = builder
                .bearer_auth(&server.token)
                .send()
                .await
                .expect("request");
            assert_eq!(response.status().as_u16(), 405);
            assert_eq!(
                response
                    .headers()
                    .get("allow")
                    .and_then(|v| v.to_str().ok()),
                Some("POST")
            );
        }
    }

    #[tokio::test]
    async fn unknown_path_is_not_found() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(format!("{}/admin", server.base_url))
            .bearer_auth(&server.token)
            .json(&request("server/discover", json!({})))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("content-type", "application/json")
            .body("x".repeat(MAX_BODY_BYTES + 1))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 413);
    }

    /// 앱이 응답 없이 요청을 버려도 클라이언트는 매달리지 않고 에러를 받는다.
    #[tokio::test]
    async fn an_unanswered_request_becomes_an_internal_error() {
        let server = TestServer::start_dropping_requests().await;
        let (status, body) = server.call(request("tools/list", json!({}))).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["error"]["code"], INTERNAL_ERROR);
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("without responding")),
            "unexpected error: {body}"
        );
    }

    /// `Content-Length`가 없는(chunked) 본문도 상한에서 끊긴다 — 헤더로 크기를 알 수 없어도
    /// 읽는 양 자체가 제한된다.
    #[tokio::test]
    async fn chunked_body_over_the_cap_is_rejected() {
        let server = TestServer::start(HashMap::new()).await;
        let chunk = Bytes::from(vec![b'x'; 64 * 1024]);
        let chunks = iced::futures::stream::iter(
            (0..=MAX_BODY_BYTES / chunk.len()).map(move |_| Ok::<_, std::io::Error>(chunk.clone())),
        );
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("content-type", "application/json")
            .body(reqwest::Body::wrap_stream(chunks))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 413);
    }

    #[tokio::test]
    async fn malformed_json_is_a_parse_error() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("content-type", "application/json")
            .body("{not json")
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 200);
        let body: Value = response.json().await.expect("json");
        assert_eq!(body["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let server = TestServer::start(HashMap::new()).await;
        let (_, body) = server.call(request("resources/list", json!({}))).await;
        assert_eq!(body["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn notification_gets_an_empty_accepted_response() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .json(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 202);
        assert!(response.text().await.expect("body").is_empty());
    }

    #[tokio::test]
    async fn a_notification_gets_no_body_even_with_a_bad_version() {
        // 버전 검사가 notification 검사보다 앞에 오면 id 없는 요청에 `"id": null` 에러
        // 본문이 돌아간다 — JSON-RPC 2.0이 금지하는 응답이다.
        let server = TestServer::start(HashMap::new()).await;
        for headers in [Some("1999-01-01"), Some(PROTOCOL_MODERN), None] {
            let mut builder = TestServer::client()
                .post(server.url())
                .bearer_auth(&server.token);
            if let Some(version) = headers {
                builder = builder.header("mcp-protocol-version", version);
            }
            let response = builder
                .json(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/initialized",
                    "params": {
                        "_meta": { "io.modelcontextprotocol/protocolVersion": PROTOCOL_LEGACY },
                    },
                }))
                .send()
                .await
                .expect("request");

            assert_eq!(response.status().as_u16(), 202, "{headers:?}");
            assert!(
                response.text().await.expect("body").is_empty(),
                "{headers:?}"
            );
        }
    }

    #[tokio::test]
    async fn protocol_version_header_mismatch_is_a_bad_request() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("mcp-protocol-version", PROTOCOL_MODERN)
            .json(&request(
                "ping",
                json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": PROTOCOL_LEGACY } }),
            ))
            .send()
            .await
            .expect("request");

        assert_eq!(response.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn unsupported_protocol_version_is_reported_with_alternatives() {
        let server = TestServer::start(HashMap::new()).await;
        let response = TestServer::client()
            .post(server.url())
            .bearer_auth(&server.token)
            .header("mcp-protocol-version", "1999-01-01")
            .json(&request("ping", json!({})))
            .send()
            .await
            .expect("request");

        let body: Value = response.json().await.expect("json");
        assert_eq!(body["error"]["code"], -32022);
        assert_eq!(body["error"]["data"]["supported"][0], PROTOCOL_MODERN);
    }

    #[tokio::test]
    async fn tools_list_is_answered_by_the_app() {
        let answers = HashMap::from([(
            "tools/list",
            McpOutcome::ToolList(vec![json!({ "name": "list_sessions" })]),
        )]);
        let server = TestServer::start(answers).await;

        let (status, body) = server.call(request("tools/list", json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["tools"][0]["name"], "list_sessions");
    }

    #[tokio::test]
    async fn tool_payload_is_wrapped_as_a_text_block() {
        let answers = HashMap::from([(
            "list_sessions",
            McpOutcome::Payload(json!({ "sessions": [] })),
        )]);
        let server = TestServer::start(answers).await;

        let (_, body) = server
            .call(request(
                "tools/call",
                json!({ "name": "list_sessions", "arguments": {} }),
            ))
            .await;

        let text = body["result"]["content"][0]["text"]
            .as_str()
            .expect("text block");
        let payload: Value = serde_json::from_str(text).expect("payload json");
        assert!(payload["sessions"].is_array());
    }

    #[tokio::test]
    async fn denied_tool_is_reported_as_method_not_found() {
        let answers = HashMap::from([(
            "list_sessions",
            McpOutcome::Denied("Not permitted at the current tier".to_string()),
        )]);
        let server = TestServer::start(answers).await;

        let (_, body) = server
            .call(request("tools/call", json!({ "name": "list_sessions" })))
            .await;

        assert_eq!(body["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn tool_failure_is_an_is_error_result() {
        let answers = HashMap::from([(
            "list_sessions",
            McpOutcome::Failure("session not found".to_string()),
        )]);
        let server = TestServer::start(answers).await;

        let (_, body) = server
            .call(request("tools/call", json!({ "name": "list_sessions" })))
            .await;

        assert_eq!(body["result"]["isError"], true);
        assert!(body.get("error").is_none());
    }

    #[tokio::test]
    async fn unknown_tool_never_reaches_the_app() {
        let server = TestServer::start(HashMap::new()).await;
        let (_, body) = server
            .call(request("tools/call", json!({ "name": "drop_database" })))
            .await;

        assert_eq!(body["error"]["code"], METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn bind_failure_is_reported() {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let port = occupied.local_addr().expect("addr").port();

        let (output, mut events) = iced_mpsc::channel(4);
        serve(
            McpServerConfig {
                port,
                token: "t".repeat(64),
            },
            output,
        )
        .await;

        match events.next().await {
            Some(McpEvent::BindFailed(message)) => {
                assert!(message.contains(&port.to_string()), "{message}");
            }
            other => panic!("expected BindFailed, got {other:?}"),
        }
    }

    #[test]
    fn origin_matrix() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://127.0.0.1")));
        assert!(origin_allowed(Some("http://127.0.0.1:47355")));
        assert!(origin_allowed(Some("http://localhost:3000")));
        assert!(origin_allowed(Some("http://[::1]")));
        assert!(origin_allowed(Some("http://[::1]:8080")));

        assert!(!origin_allowed(Some("http://evil.com")));
        assert!(!origin_allowed(Some("http://localhost.evil.com")));
        assert!(!origin_allowed(Some("http://127.0.0.1.evil.com")));
        assert!(!origin_allowed(Some("https://localhost")));
        assert!(!origin_allowed(Some("null")));
        assert!(!origin_allowed(Some("file://")));
        // 포트 접미사는 숫자만이다 — 자격증명·경로가 붙은 authority는 다른 출처다.
        assert!(!origin_allowed(Some("http://localhost:")));
        assert!(!origin_allowed(Some("http://localhost:3000/path")));
        assert!(!origin_allowed(Some("http://127.0.0.1:47355@evil.com")));
    }

    #[test]
    fn authorization_header_matrix() {
        let token = "c".repeat(64);
        let with = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                header::HeaderValue::from_str(value).expect("header"),
            );
            authorized(&token, &headers)
        };

        assert!(with(&format!("Bearer {token}")));
        assert!(
            with(&format!("bearer {token}")),
            "scheme is case-insensitive"
        );
        assert!(!with(&token), "raw token without a scheme is rejected");
        assert!(!with(&format!("Basic {token}")));
        assert!(!with("Bearer "));
        assert!(!with(&format!("Bearer {}", "d".repeat(64))));
        assert!(!authorized(&token, &HeaderMap::new()));
    }
}
