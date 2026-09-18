//! 로컬 MCP(Model Context Protocol) 서버.
//!
//! 외부 AI 에이전트(Claude Code 등)가 `POST http://127.0.0.1:<port>/mcp`로 앱의 실행 구성과
//! 세션을 조회·조작한다. 서버는 사용자가 설정에서 켤 때만 뜨고, 노출되는 툴은 권한 단계
//! (`McpPermission`)로 제한된다.
//!
//! # 요청 브리지
//!
//! 앱의 모든 상태(구성·세션·PTY·페인)는 iced update 루프가 단독 소유한다. HTTP 태스크는
//! 상태를 직접 읽지 않고 `McpRequest`를 루프로 보내 GUI 버튼과 **같은 핸들러**를 통과시킨다.
//! 덕분에 락이 필요 없고, MCP 경로와 GUI 경로의 동작이 갈라질 수 없다.
//!
//! ```text
//! hyper task ──mpsc──▶ Subscription ──▶ Message::Mcp ──▶ update() ──mpsc(1)──▶ hyper task
//! ```

mod http;
mod protocol;
mod token;
mod tools;

pub use token::{
    load_or_create as load_or_create_token, mask as mask_token, regenerate as regenerate_token,
};
pub use tools::{MAX_SEARCH_MATCHES, McpOp, McpPermission, tools_for};

use iced::futures::Stream;
use iced::stream;
use serde_json::Value;
use tokio::sync::mpsc;

/// 기본 리스닝 포트. IANA 미할당 동적 포트 범위(49152 미만이지만 등록되지 않은 대역)에서
/// 흔한 개발 서버 포트(3000·5173·8080 등)와 겹치지 않는 값을 골랐다.
pub const DEFAULT_PORT: u16 = 47355;

/// 서버 구동 파라미터. iced subscription의 정체성(`Hash`)으로 쓰이므로, 이 값이 바뀌면
/// 낡은 서버가 drop되고 새 설정으로 다시 뜬다 — 별도 shutdown 채널이 필요 없다.
///
/// 권한 단계와 환경변수 노출 토글은 **의도적으로 여기 없다**. 두 값은 앱 상태에만 존재하고
/// 매 요청마다 브리지 뒤에서 판정되므로, 사용자가 단계를 바꿔도 서버를 재시작할 필요가 없고
/// 판정 기준이 두 곳으로 복제되지도 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct McpServerConfig {
    pub port: u16,
    pub token: String,
}

/// 앱이 브리지로 돌려주는 결과.
#[derive(Debug, Clone)]
pub enum McpOutcome {
    /// `tools/list` — 현재 권한 단계에서 노출되는 툴 목록.
    ToolList(Vec<Value>),
    /// 툴 실행 성공 payload.
    Payload(Value),
    /// 툴이 일을 못 했다(대상 없음, 실행 실패). 프로토콜 위반이 아니므로 `isError` 결과가 된다.
    Failure(String),
    /// 현재 권한 단계가 이 툴을 허용하지 않는다.
    Denied(String),
}

/// update 루프가 처리할 요청. 응답은 정확히 한 번 `respond`로 돌려준다.
#[derive(Debug, Clone)]
pub struct McpRequest {
    op: McpOp,
    /// 용량 1 bounded 채널. `Message`가 `Clone`을 요구해 `oneshot::Sender`를 쓸 수 없다.
    reply: mpsc::Sender<McpOutcome>,
}

impl McpRequest {
    pub fn op(&self) -> &McpOp {
        &self.op
    }

    /// 결과를 요청자에게 돌려준다. 클라이언트가 이미 끊겼거나 타임아웃된 경우 채널이 닫혀
    /// 있는데, 그건 정상 종료 경로이므로 조용히 버린다.
    pub fn respond(self, outcome: McpOutcome) {
        let _ = self.reply.try_send(outcome);
    }

    /// 앱 핸들러를 HTTP 계층 없이 검증하기 위한 요청/응답 짝.
    #[cfg(test)]
    pub fn channel_for_test(op: McpOp) -> (Self, mpsc::Receiver<McpOutcome>) {
        let (reply, answers) = mpsc::channel(1);
        (Self { op, reply }, answers)
    }
}

/// subscription이 앱으로 올려보내는 이벤트.
#[derive(Debug, Clone)]
pub enum McpEvent {
    /// 리스너가 떴다. 포트 0을 요청했을 수도 있어 실제 바인드된 포트를 싣는다.
    Started { port: u16 },
    /// 바인드 실패(포트 점유 등). 스트림은 여기서 끝나고 사용자가 설정을 바꿀 때까지 멈춘다.
    BindFailed(String),
    /// 처리해야 할 요청.
    Request(McpRequest),
}

/// MCP 서버 스트림. `Subscription::run_with`의 builder에서 호출한다.
pub fn server(config: McpServerConfig) -> impl Stream<Item = McpEvent> {
    stream::channel(100, move |output| async move {
        http::serve(config, output).await;
    })
}
