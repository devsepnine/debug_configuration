# MCP 연동 가이드

> Last Updated: 2026-09-18
> 대상: `run_config_manager` (로컬 MCP 서버)

앱이 로컬 HTTP MCP 서버를 열어, 외부 AI 에이전트(Claude Code 등)가 구성 조회 · 실행 ·
출력 읽기 · 구성 편집을 하게 한다. 앱이 LLM을 호출하는 기능은 없다 — 앱은 서버 쪽이다.

기본값은 **서버 꺼짐 · Read only · 환경변수 값 마스킹**이다. 사용자가 켜지 않으면 리스너가
뜨지 않는다.

---

## 1. 켜기

Settings → `AI (MCP)` 섹션.

| 항목 | 기본값 | 뜻 |
|---|---|---|
| `Enable local MCP server` | off | 리스너 시작 |
| `Permission` | `Read only` | 노출할 툴 단계 (아래 3장) |
| `Port` | `47355` | `127.0.0.1`의 바인드 포트 |
| `Expose environment variable values` | off | off면 값이 `<hidden>`으로 나간다 (5장) |
| `Token` | MCP가 켜져 있고 토큰이 없으면 생성 | 마스킹 표시 · 복사/재생성 아이콘 버튼 |
| `Register with Claude Code` | — | `claude mcp add` 명령 · 복사 아이콘 버튼 |

토큰 파일은 **MCP가 켜져 있고 토큰이 없을 때** 만들어진다 — 앱 시작 시점과 설정 `OK`를
누를 때마다 이 조건을 다시 본다. 그래서 토큰 로드가 한 번 실패해도 `OK` 한 번으로 다시
시도되고, 파일을 지우면 다음 `OK`에 새로 생긴다. MCP를 켠 적이 없으면 `Token` 행은
`Not created yet`이고 복사·재생성 아이콘은 눌리지 않는다 — 이 기능을 쓰지 않는 사용자의 홈에는
비밀 파일이 남지 않는다.

설정 모달은 staging 방식이다 — `OK`를 눌러야 적용되고 `Cancel`은 폐기한다. 적용 후
`Status` 행이 결과를 싣는다:

- `Disabled` — 꺼짐
- `Starting…` — 바인드 결과 대기
- `Listening on http://127.0.0.1:47355/mcp` — 정상
- `Failed: …` — 포트 점유, 토큰 파일 접근 불가 등

### 클라이언트 등록

```bash
claude mcp add --transport http runconfig http://127.0.0.1:47355/mcp \
  -H "Authorization: Bearer <token>"
```

`Register with Claude Code` 행의 복사 아이콘이 **실제 토큰이 박힌** 이 명령을 클립보드에
넣는다(화면에 보이는 명령의 토큰은 마스킹돼 있다).

**주의 — 셸 히스토리**: 이 명령을 그대로 터미널에 붙이면 토큰이 셸 히스토리 파일에 평문으로
남는다. 토큰 파일을 `0600`으로 두는 것과 어긋나므로, 다음 중 하나를 쓴다.

- 명령 앞에 공백 하나를 넣어 실행한다 (`HISTCONTROL=ignorespace`/zsh `setopt histignorespace`가 켜져 있을 때만 유효하다)
- 등록 후 히스토리에서 해당 줄을 지운다
- 등록 후 Settings의 `Token` 행에서 재생성 아이콘으로 토큰을 새로 만들고 다시 등록한다 (히스토리에 남은 값이 무효가 된다)

`Port`를 바꾸거나 토큰을 재생성하면 등록해 둔 명령이 무효가 된다 — 옛 토큰은 `401`을
받는다. 클라이언트를 다시 등록한다.

---

## 2. 보안 경계

| 통제 | 동작 |
|---|---|
| 바인드 주소 | `127.0.0.1`만. `0.0.0.0`으로 바인드하는 경로가 코드에 없다 |
| 엔드포인트 | `POST /mcp`만. `GET`/`DELETE`는 `405`(SSE·resumability 미제공), 다른 경로는 `404` — 단 **인증이 먼저**라, 토큰 없이 두드리면 어느 메서드·경로든 `401`이고 엔드포인트의 존재 여부가 드러나지 않는다 |
| 인증 | `Authorization: Bearer <64 hex>`, 상수시간 비교. 없거나 틀리면 `401` |
| Origin | 헤더 부재는 허용(비브라우저 클라이언트의 정상 요청). 있으면 `http://127.0.0.1[:포트]` · `http://localhost[:포트]` · `http://[::1][:포트]`만 허용하고 그 외는 `403` — 브라우저의 `null` 출처와 http 이외 스킴도 거부한다 (DNS rebinding 방어). 이 검사는 인증보다 **앞**이다 |
| 요청 크기 | body 1 MiB 초과 시 `413` |
| 응답 시간 | 앱이 10초 안에 답하지 않으면 `-32603`. 클라이언트가 무한 대기하지 않는다 |
| 클라이언트 알림 | `id` 없는 JSON-RPC 요청(JSON-RPC notification)은 `202`로 끝난다. 서버가 클라이언트에게 보내는 MCP notification은 제공하지 않는다(9장) |

### 토큰 파일

- 경로: `~/.run_config_mcp_token`
- unix 퍼미션 `0600`. 느슨한 퍼미션으로 발견되면 로드 시 조인다
- **Windows에는 파일 ACL을 직접 걸지 않는다** — 사용자 프로필 디렉터리의 ACL에 의존한다.
  프로필 디렉터리를 다른 사용자와 공유하거나 ACL을 완화한 환경에서는 이 전제가 성립하지
  않으므로, 그런 환경에서는 서버를 켜지 않는 편이 낫다
- 앱 설정 JSON에는 토큰이 들어가지 않는다 — 설정 파일을 백업·공유해도 토큰이 새지 않는다

### 감사 가시성

변경성 MCP 호출은 앱 상태바에 `[MCP]` 접두사가 붙은 한 줄을 남긴다. 사용자가 자기 조작과
에이전트의 조작을 구별하는 유일한 표시다.

```
[MCP] Running: build
[MCP] Rerunning: build
[MCP] Stopped session: build
[MCP] Created configuration 'api'
[MCP] Updated configuration 'api'
[MCP] Update left configuration 'api' unchanged
[MCP] Deleted configuration 'api'
[MCP] Denied delete_configuration: beyond the current permission tier
[MCP] Rejected update_configuration: request_id reused for a different target
[MCP] Refused to update configuration: A configuration named 'api' already exists
[MCP] Failed to save configurations: <사유>
```

거절도 남는다 — 권한 밖 삭제 시도나 파괴적 툴의 대상 혼동은 성공한 삭제보다 감사 가치가
낮지 않다.

**상태바는 한 줄뿐이다.** 다음 조작이 이 줄을 덮으므로 이것은 로그가 아니라 마지막 사건의
표시다 — 사용자가 앱을 보지 않는 동안 벌어진 여러 조작 가운데 마지막 하나만 남는다. 편집이
디스크에 닿지 못한 경우만 예외적으로 성공 문구를 덮어쓴다(`Failed to save configurations`) —
성공한 저장은 조용해서 방금 남은 감사 줄을 지우지 않는다.

---

## 3. 권한 단계

| 단계 | 추가되는 툴 |
|---|---|
| `Read only` (기본) | `list_configurations` · `get_configuration` · `list_sessions` · `read_session_output` · `search_session_output` |
| `Execute` | + `run_configuration` · `stop_session` · `rerun_session` · `send_session_input` |
| `Edit` | + `create_configuration` · `update_configuration` · `delete_configuration` |

상위 단계는 하위 단계의 모든 툴을 포함한다. `tools/list`가 단계로 걸러지므로 허용되지 않은
툴은 **목록에 아예 보이지 않고**, `tools/call`도 독립적으로 다시 검사한다(목록을 캐시한
클라이언트 대비). 단계 밖 호출은 `-32601`을 받는다.

`Execute`는 사실상 임의 명령 실행이고 `Edit`은 사용자 데이터 변경이다. 필요한 만큼만 올린다.

---

## 4. 툴

### Read only

**`list_configurations`** — 저장된 구성 전체의 `id` · `name` · `type` ·
`working_directory` · `environment_variable_keys`. **값은 마스킹된 형태로도 실리지 않는다**
(키만). 값이 필요하면 구성 하나를 `get_configuration`으로 읽는다.

**`get_configuration(configuration)`** — 구성 하나의 전체 상세. `configuration`은 id(UUID)
또는 **정확한** 이름이다. `type_data`가 타입별 필드를 그대로 싣는다 — 편집 툴에 넘길
`type_data`는 이 응답에서 복사해 고치는 것이 정해진 사용법이다.

**`list_sessions`** — 세션별 `state` · `exit_code` · `error` · `started_at_unix_ms` ·
`duration_ms` · `line_count`.

`state` 값: `running` · `succeeded` · `failed`(종료 코드 있음) · `stopped`(사용자·에이전트가
중지) · `errored`(종료 코드가 아예 없는 실패 — 사유는 `error` 필드).

**`read_session_output(session_id, tail_lines?, since_line_id?)`** — ANSI를 제거한 평문.
`tail_lines` 기본 200줄, 최대 5000줄. 응답은 `text` · `last_line_id` · `buffered_lines` ·
`returned_lines` · `omitted_lines`.

이어 읽기: 앞선 호출의 `last_line_id`를 `since_line_id`로 넘기면 그 뒤에 추가된 줄만 온다.
줄 id는 버퍼 축출을 넘어 안정적이지만, 제자리에서 덮어써지는 줄(진행 표시줄)은 id를 유지한다.

**`last_line_id`는 돌려준 줄이 없으면 `null`이다** — 새 줄이 아직 없는 폴링에서 정상으로 나오는
값이다. 커서를 이 값으로 무조건 갈아 끼우면 `null`이 되어 다음 호출이 버퍼 앞쪽부터 다시
읽는다. 새 값이 `null`이면 앞선 커서를 그대로 둔다.

**`search_session_output(session_id, query, regex?)`** — 대소문자 무시 부분 문자열이 기본,
`regex: true`면 정규식. 응답은 `total_matches` · `matched_lines` · `truncated`.

매치는 줄 단위로 접힌다 — 한 줄에 여러 번 맞으면 항목 하나에 `match_count`가 올라간다.
그래서 두 수가 다른 것을 센다: `total_matches`는 **매치 개수**, `matched_lines`와 `truncated`가
따르는 상한 500줄은 접힌 **줄 개수**다. 한 줄에 매치가 몰린 검색은 `total_matches`가
500을 넘으면서도 `truncated: false`일 수 있다.

### Execute

**`run_configuration(configuration, request_id?)`** — 세션이 만들어지는 즉시 반환하고 실행은
앱 안에서 계속된다. 진행·최종 상태·종료 코드는 `read_session_output`/`list_sessions`로
폴링한다. **시작조차 못 한 명령도 이 호출의 오류가 아니라 `errored` 상태의 세션으로**
보고된다. compound 구성은 멤버당 세션 하나를 만들므로 `session_id`가 아니라 `sessions`
배열을 읽는다.

compound에서 **돌릴 수 없는 멤버는 조용히 빠진다** — 그 사이 삭제된 멤버와, 멤버 자신이
compound인 경우다(중첩 실행을 하지 않는다). 그래서 `sessions`가 `members`보다 짧을 수 있고,
응답에는 빠진 개수가 없다(앱 상태바에는 `N task(s), M skipped`가 남는다). 전부 실행됐는지
확인하려면 호출자가 `sessions`의 길이를 멤버 수와 직접 견준다. 돌릴 멤버가 하나도 없으면
성공이 아니라 실패로 답한다.

**`stop_session(session_id)`** — 멱등이다. 이미 끝난 세션은 오류가 아니라 최종 상태로 답한다.

**`rerun_session(session_id, request_id?)`** — 같은 페인에서 다시 실행한다.

- **출력을 지우지 않는다** — 새 실행이 옛 출력 아래에 붙는다
- 세션 id가 **새로 생긴다**. 넘긴 id는 더 이상 존재하지 않으므로, 응답의 `session_id`를
  폴링한다
- 응답의 `previous_last_line_id`를 그대로 `since_line_id`로 넘기면 새 실행분만 읽힌다.
  `null`인 두 경우: 중복 흡수 응답(6장)에서는 그 시점에 다시 재면 새 실행이 이미 찍은 줄을
  가리켜 그만큼을 조용히 건너뛰게 되므로 싣지 않고, 이전 실행의 출력 버퍼가 비어 있으면
  경계로 쓸 줄 자체가 없다. 어느 쪽이든 `since_line_id`를 생략하고 전체를 읽는다

**`send_session_input(session_id, text, request_id?)`** — 실행 중인 세션의 stdin에 한 줄을
쓴다(입력 바에 타이핑한 것과 같다).

- 줄 종결자는 앱이 붙인다 → `text`에 개행(`\n`·`\r`)이 있으면 `-32602`. 여러 단계 프롬프트는
  줄마다 호출 하나로 답한다. 빈 문자열은 개행만 보낸다
- `text`는 최대 **4096 바이트**(문자 수가 아니다 — 비 ASCII 줄은 문자 수보다 먼저 상한에 닿는다)
- 끝난 세션에 쓰면 실패한다
- 응답 `status`: `delivered`(전달됨) · `pending`(프로그램이 아직 stdin을 읽지 않아 쓰기가
  진행 중 — 다시 보내면 같은 줄이 두 번 전달된다) · `duplicate`(6장)

### Edit

세 툴 모두 성공하고 **실제로 무엇이 달라졌으면** 구성 목록 전체를 디스크에 저장한다(7장).
아무것도 바꾸지 않은 `update`는 저장을 발행하지 않는다(`changed: false`).

응답 모양은 세 툴이 같다 — `configuration_id` · `configuration`(편집 뒤의 상세) ·
`deduplicated`. `configuration`은 목록에 그 구성이 없으면 `null`이다(`delete`가 언제나 이쪽이다).
**`configuration`도 마스킹 토글을 따른다**(5장) — 환경변수 값을 실어 성공한 쓰기조차 기본
설정에서는 값이 `<hidden>`으로 돌아온다. 방금 쓴 값을 되읽어 확인하는 용도로는 쓸 수 없다.

**`create_configuration(name, working_directory, type_data, environment_variables?, request_id?)`**

- `id`는 앱이 정해 응답에 싣는다. 호출자가 줄 수 없다
- `name`은 중복일 수 없고 **UUID 모양일 수 없다** — 이름이 다른 툴에서 구성을 지목하는
  주소이기 때문이다
- `working_directory`는 만드는 시점에 존재를 검사하지 않고 **빈 값만 거절한다**. 존재하지 않는
  디렉터리는 **실행 시점에** 거절된다 — 실행은 스폰 전에 그 경로가 디렉터리인지 보고, 아니면
  세션이 `Working directory does not exist or is not a directory: …`를 사유로 실패한다
  (`state`는 `errored`, `exit_code`는 `null`, 사유는 `error`에 실린다 — `exit_code` 키 자체는
  언제나 있다). 그 검사가 생기기 전에는 기본 PTY 경로가 그 값을 버려 프로그램이 사용자의 홈
  디렉터리에서 돌았고, 세션은 그 명령이 거기서 낸 결과를 보고했다 — 홈에서 성공하는 명령이면
  `succeeded`였다. 두 타입이 예외다:
  **Compound**는 멤버를 각자의 디렉터리에서 실행하므로 이 값이 쓰이지 않고, **Node**는 이
  값을 `type_data.project_directory`에서 파생시킨다(무엇을 실어도 덮인다). Node를 하위
  디렉터리에서 돌리려면 `project_directory`를 그 디렉터리로 잡는다 — 대신 Node에서는
  `project_directory`가 빈 값일 수 없다
- compound의 `members`에 같은 구성을 두 번 실을 수 없다. 실행은 항목마다 세션을 띄우므로
  중복은 한 번 눌러 두 세션이 되고, 앱의 멤버 목록은 체크박스여서 그 상태를 되돌릴 수 없다.
  멤버는 자기 자신도, 다른 compound도 될 수 없다
- `type_data`는 `get_configuration`의 것을 복사해 고친다. 미지 필드는 거절된다 —
  `members`를 `member`로 잘못 쓰면 조용히 빈 목록이 되는 대신 `-32602`가 온다
- Node 구성을 만들어도 **package.json을 찾아 스크립트 목록을 채우지 않는다**. 그 스캔은
  하위 방향 동기 재귀 디렉터리 워크여서 호출자가 넘긴 경로로 update 루프 안에서 돌면 그 트리를
  훑는 동안 앱이 응답하지 않는다 — 깊이 5와 제외 목록이 있어 무한하지는 않지만 비용은 호출자가
  고른 트리에 비례한다(`update`가 같은 종류의 이유로 재스캔하지 않는다 — 7장). 목록은 사용자가 그 구성을
  앱에서 고를 때 채워지고, `script_name`은 그 목록과 무관하게 실을 수 있다

**`update_configuration(configuration, …, request_id?)`** — 실린 필드만 바꾸고, 최소 하나는
있어야 한다.

- 환경변수는 통째로 교체하지 않는다 — `set_environment_variables`(맵) ·
  `remove_environment_variables`(키 배열)로 키 단위로만 편집한다. 읽은 값이 마스킹돼
  있을 수 있어(5장) 되돌려 쓰는 것을 정상 사용법으로 만들지 않기 위한 것이다
- 한 키가 양쪽에 다 실리면 **설정이 이긴다**(제거를 먼저 적용한다)
- `type_data`는 실리면 **객체 전체를 교체한다**
- `working_directory`의 빈 값은 **결과 타입**을 기준으로 판정한다 — 패치를 적용한 뒤의
  타입이 Compound나 Node면 허용되고, 그 밖이면 거절된다. Node로 남거나 Node가 되는 패치는
  `project_directory`에서 값을 파생시키므로 이 필드를 실어도 덮인다
- 응답의 `changed`는 요청의 모양이 아니라 **실제 변화**를 뜻한다. 설정되지 않은 키를
  제거하거나 같은 값을 다시 넣는 호출은 `changed: false`이고, 저장도 발행하지 않는다.
  중복 흡수 응답에서는 `null`이다 — 그 답은 첫 호출을 설명하는 것이어서, `false`로 두면
  일어난 변경을 일어나지 않은 것으로 읽게 한다

**`delete_configuration(configuration, request_id?)`**

- **되돌릴 수 없고 확인 모달을 거치지 않는다**(앱의 Delete 버튼은 거친다)
- 이 구성을 멤버로 두고 있던 compound들은 그 멤버를 잃는다. 응답의
  `referencing_compounds`가 그 이름들을 싣는다 — 사용자가 확인 모달에서 봤을 정보다.
  중복 흡수 응답에서는 `null`이다(첫 호출이 이미 걷어냈으므로 `[]`는 "참조가 없었다"로
  오독된다)
- **`referencing_compounds`는 파급의 전부가 아니다.** 이 구성으로 돌고 있던 세션은 계속 돌고
  페인도 열려 있으며, 그 세션의 `rerun_session`은 이후 계속 실패한다(9장). 목록이 비어 있다고
  파급이 없는 것이 아니므로, 실행 중인 구성을 지울 때는 `list_sessions`로 먼저 확인하고
  필요하면 `stop_session`을 부른다

### 호출자 문자열 상한

| 대상 | 상한 |
|---|---|
| `name` · `configuration` | 200자 |
| `working_directory` | 4096자 |
| 환경변수 이름 | 256자 |
| 환경변수 값 | 32768자 |
| `request_id` | 128자 |
| `search_session_output`의 `query` | 4096자 |
| `send_session_input`의 `text` | 4096 **바이트** |
| `type_data` 안의 경로 (`command` · `script_path` · `project_directory` · `jar_path` 등) | 4096자 |
| `type_data` 안의 이름 (`main_class` · `script_name` · compound의 `workspace`) | 200자 |
| `type_data` 안의 그 밖의 자유 텍스트 (`arguments` · `vm_options` · `script_text` · `classpath` 등) | 32768자 |
| compound `members` 개수 | 1024개 |

`text`만 바이트, 나머지는 **문자** 수다 (JSON Schema `maxLength`의 정의와 맞춘 것이다).

이 상한은 **앱에는 걸려 있지 않다**. 사용자가 앱에서 넣은 값은 위 상한보다 길 수 있고, 그러면
`get_configuration`으로 읽은 그 값을 아무것도 바꾸지 않고 되돌려 주는 `update_configuration`도
`-32602`로 거절된다 — 왕복도 쓰기이므로 상한을 다시 통과해야 한다. 다른 필드만 고칠 때는
`type_data`를 싣지 않으면 되고, 그 값 자체를 고쳐야 하면 앱에서 값을 줄이거나 그 편집을 앱에서
하는 것이 회복 경로다.

상한은 **보내는 값**만 묶는다. 응답은 저장된 값을 그대로 싣기 때문에, 상한을 넘는 값을 가진
구성은 개명처럼 그 값을 건드리지 않는 호출에서도 매번 그 값을 되돌려 받는다.
`get_configuration`도 같고 그쪽은 기본 Read only 단계다. 값 없이 목록만 확인해야 하면
`list_configurations`가 값을 싣지 않는 채널이다.

`type_data` 안쪽 상한과 `text`의 바이트 상한은 스키마에 실려 있지 **않다** — 앞의 것은 스키마가
`type_data`의 모양을 열거하지 않기 때문이고(타입별 형태는 `get_configuration`이 돌려주는 그대로다),
뒤의 것은 `maxLength`가 문자 수를 뜻해서 바이트 상한을 표현할 수 없기 때문이다. 두 경우 모두
클라이언트 쪽 사전 검증이 없고 서버가 `-32602`로 거절한다.

---

## 5. 환경변수 마스킹

`Expose environment variable values`가 꺼져 있으면(기본):

- `get_configuration`의 환경변수 **값**이 `<hidden>`으로 바뀐다. 키는 그대로 보인다
- `read_session_output`에서 실행 배너(`Environment: …`)의 값이 가려진다. 가려지는 줄은
  **텍스트 모양이 아니라 앱이 찍을 때 기록한 줄 id**로 판정한다 → 프로그램이 스스로 찍은
  `Environment: …` 줄은 사용자 출력이므로 가려지지 않는다
- `search_session_output`은 가려진 줄을 결과와 `total_matches`에서 **모두** 뺀다. 가려진
  값을 검색해서 존재를 확인할 수 없다. 같은 이유로 "가려진 줄 N개" 같은 카운트를 응답에
  싣지 않는다

배너 자체는 두 번째 설정에 달려 있다. 앱의 `Show environment line on run`이 꺼져 있으면 실행
출력에 `Environment: …` 줄이 **아예 없고**, 그 상태에서는 마스킹 토글을 켜도 가릴 줄이 없어
아무 차이가 나지 않는다. 배너의 부재는 "환경변수가 없다"는 뜻이 아니다 — 환경변수를 읽는 채널은
`list_configurations`(키)와 `get_configuration`(키와 값)이고, 둘은 이 설정과 무관하다.

노출을 켠 상태에서도 배너의 값은 **표시용** 형태다. 값 안의 개행·복귀는 배너가 한 줄로 남도록
이스케이프돼 `\n`·`\r`로 보이며, 그 문자열을 그대로 되돌려 쓰면 역슬래시 두 글자가 값으로
저장된다. 배너는 사람이 읽는 산문이지 값의 출처가 아니다 — 값은 `get_configuration`으로 읽는다.

### `<hidden>`을 되돌려 쓰는 것은 거절된다

환경변수 값으로 `<hidden>`을 보내면 **토글 상태와 무관하게** 거절된다. 마스킹된 값을 읽어
그대로 되돌려 쓰면 사용자의 실제 비밀이 리터럴 `<hidden>`으로 덮이고, 그 손실은 되돌릴 수
없다. 토글에 따라 갈리게 하면 같은 요청이 앱 설정에 따라 비밀을 파괴하므로 무조건 거절한다.

실제로 값을 바꾸려면 진짜 값을 보내고, 바꾸지 않으려면 그 변수를 호출에서 빼면 된다.

---

## 6. `request_id` — 재시도 멱등성

HTTP 클라이언트는 응답을 받기 전에 연결이 끊기면 같은 POST를 재시도한다. 읽기 툴에서는
무해하지만 실행 툴은 프로세스를 두 번 띄우고 편집 툴은 구성을 두 번 만든다. 실행 3개
(`run_configuration` · `rerun_session` · `send_session_input`)와 편집 3개, 모두 **6개 툴**이
선택적 `request_id`를 받는다. `stop_session`은 설계상 멱등이어서 받지 않는다 — 이미 끝난
세션을 멈추는 호출도 성공이다.

- **창 60초.** 같은 툴 · 같은 `request_id` · 같은 대상이 창 안에 다시 오면, 실행하지 않고
  첫 호출의 결과를 그대로 돌려준다(`deduplicated: true` 또는 `status: "duplicate"`)
- **대상이 다르면 거절한다.** 같은 `request_id`를 다른 구성·다른 세션·**다른 내용**으로
  쓰면 흡수가 아니라 오류다. 첫 결과를 돌려주면 요청하지 않은 대상의 답을 받고, 새로
  실행하면 `request_id`가 약속한 멱등성이 깨진다
  - `send_session_input`은 **줄 내용**이 대상에 접혀 있다 → 멱등성을 원하면 **줄마다 새
    `request_id`**를 쓴다. 세션당 하나를 재사용하면 두 번째 줄에서 거절된다
  - `create`/`update`는 **요청 내용**이 대상에 접혀 있다 → 같은 id로 다른 패치를 보내면
    거절된다. 60초 안에 `MODE=debug` → `MODE=release`를 같은 id로 보내면 나중 값이
    조용히 버려지는 대신 오류가 온다
- **`request_id`가 없으면 창을 거치지 않는다.** 재시도는 다시 실행된다 — 그 판단은
  호출자에게 남는다
- **창이 가득 차면 거절한다.** 아직 유효한 기록 512개가 차 있으면 새 `request_id`를 실은
  호출은 실행되지 않고 오류를 받는다. 자리를 만들려고 축출하면 유효한 기록이 사라져 그
  id의 재시도가 조용히 두 번 실행되기 때문이다. 회복: 창이 비기를 기다리거나(최대 60초),
  `request_id` 없이 다시 보낸다(그 경로에는 재시도 보호가 없다)
- **상한 512는 실행·편집 툴이 함께 쓴다.** 한 툴의 범람이 다른 툴의 멱등 경로도 막는다
- **거절은 창에 기록되지 않는다.** 이름 중복 등으로 거절된 호출은 원인을 고쳐 **같은
  `request_id`로 다시** 보낼 수 있다. 아무것도 만들지 않은 요청을 기록하면 재시도가
  "이미 했다"로 흡수되어 일어나지 않은 생성이 성공으로 굳는다

---

## 7. 편집 툴이 앱에 미치는 영향

MCP 요청은 GUI 버튼과 같은 update 루프 안에서 처리되고, 실행·삭제 같은 조작은 **GUI 버튼이
부르는 것과 같은 핸들러**를 재사용한다. 락이 없고 두 경로가 갈라지지 않는 것이 이 설계의
목적이며, 그 대가로 에이전트의 호출이 사용자 화면에 보인다. 다만 재사용은 전부가 아니다 —
확인 모달, 사용자 입력 드래프트, 동기 디스크 스캔처럼 10초 요청 안에서 성립하지 않거나
에이전트에게 의미가 없는 부분은 의도적으로 빠져 있고, 아래 목록이 그 차이를 싣는다.

- **저장은 목록 전체를 쓴다** — MCP 편집 하나가 사용자의 저장하지 않은 GUI 편집까지 함께
  커밋한다. 부분 저장 경로가 없고, 반대 방향(사용자 편집을 버리는 저장)이 더 나쁘다
- **저장소를 읽지 못했으면 편집을 거절한다.** 로드가 실패한 상태에서 편집을 받아 주면
  빈 목록에 만든 구성 하나가 저장소 파일을 덮어쓰고, 저장은 백업을 남기지 않아 수기로
  복구할 수 있었던 파일이 사라진다. 로드가 끝나기 전에도 거절한다
- **환경변수가 실제로 바뀌면** 그 구성에 대해 열려 있던 env 모달과 벌크 편집 드래프트를
  버린다. 남겨 두면 사용자의 Confirm 한 번이 에이전트의 변경을 조용히 되돌린다. 개명만
  하는 호출은 드래프트를 건드리지 않는다
- **디렉터리·타입이 바뀌면** Node 메타데이터 캐시(스크립트·package.json 목록)를 **버린다**.
  낡은 목록을 남기면 편집기가 옛 프로젝트의 스크립트를 제시하고, 사용자가 그중 하나를 고르면
  새 프로젝트에 없는 스크립트가 구성에 박힌다. 여기서 다시 재지는 않는다 — 그 스캔은 동기
  파일 I/O여서 호출자가 넘긴 경로가 그동안 update 루프를 붙잡는다(`package.json`을 크기 상한
  없이 읽으므로 비용은 그 파일 크기에 비례한다). 목록은 사용자가 그 구성을 앱에서 고를 때
  채워진다
- **실행은 사용자 화면을 옮긴다** — `Sessions` 탭으로 전환하고 활성 워크스페이스에 페인을
  연다. 워크스페이스 이름을 가진 compound는 새 워크스페이스 탭을 만들고 전환한다
- **생성은 편집기 선택을 옮기지 않는다** — 새 구성은 목록 하단에 나타나고, 사용자가 편집 중이던
  구성이 그대로 선택돼 있다. 선택을 옮기면 편집기가 새 구성을 렌더하고 이름 입력도 같은 인덱스로
  되쓰이므로 타이핑 중이던 사용자의 다음 키가 에이전트의 구성에 들어간다. 대가로 새 구성은
  사용자가 목록에서 직접 고를 때까지 편집기에 열리지 않으며, Node 메타데이터도 그때 채워진다
- 하드 실패 시 사용자의 stdin 입력 드래프트는 복원하지 않는다 — 에이전트가 보낸 텍스트가
  사용자 입력창에 남지 않게 하는 의도적 예외다

---

## 8. 오류 계약

| 코드 | 뜻 |
|---|---|
| `-32700` / `-32600` | JSON 파싱 실패 / JSON-RPC 요청 형태 위반 |
| `-32601` | 없는 메서드·툴, 또는 **현재 권한 단계 밖의 툴** |
| `-32602` | 인자 검증 실패 (상한 초과, 개행 포함, 미지 필드, 빈 값 등) |
| `-32603` | 앱이 10초 안에 답하지 않았거나 요청을 버렸다 |
| `-32022` | 지원하지 않는 프로토콜 리비전 (`data.supported`에 지원 목록) |

**실행·편집 실패는 JSON-RPC 오류가 아니라 `isError: true`인 툴 결과**로 온다 — 모델이
텍스트로 읽어야 다음 행동을 정할 수 있기 때문이다. "구성을 찾을 수 없다", "세션이 실행 중이
아니다", "이름이 이미 있다" 같은 사유가 여기 실린다.

프로토콜 리비전은 `2026-07-28`(modern)과 `2025-11-25`(legacy)를 지원한다.
`MCP-Protocol-Version` 헤더가 있으면 body의 버전과 일치를 검증하고, 어긋나면 `400`이다.

---

## 9. 알려진 한계

미제공 기능이 아니라 **알고 쓰지 않으면 손실이 되는 것들**을 함께 적는다.

### 제공하지 않는 것

- MCP resources · 서버→클라이언트 notification · SSE 스트리밍 · stdio 전송
  (클라이언트가 보내는 `id` 없는 JSON-RPC notification은 2장대로 받는다)
- 실행 중인 세션에 대한 푸시 알림이 없다 — 진행은 폴링으로 읽는다
- 에이전트가 시작할 수 있는 세션 수에 상한이 없다. `Execute` 단계가 opt-in이라는 사실이
  현재의 유일한 경계다
- 재시도 창은 인메모리다. 앱을 다시 시작하면 비고, 프로세스 간에 공유되지 않는다

### 남아 있는 위험

- **개명 뒤 `list_sessions`의 이름은 낡는다.** 세션은 구성을 **id**로 잇기 때문에 개명은 그
  결합을 끊지 않는다 — 개명 뒤에도 `rerun_session`과 앱의 Rerun 버튼은 그 구성을 그대로
  따라간다. 다만 `config_name`은 **세션 객체가 만들어진 시점**의 값이고 재실행은 그 값을
  갱신하지 않으므로(`started_at`만 새로 찍는다) 개명 뒤에는 낡은 이름을 계속 보고한다.
  **그 이름만으로 구성을 되찾을 수는 없다** — 낡은 이름은 아무것도 가리키지 않거나 그 이름을
  물려받은 **다른** 구성을 가리킨다. 세션을 돌려주는 응답 어디에도 구성 id가 실리지 않으므로
  세션만 쥐고는 되이을 수 없다. **회복 경로는 구성 id다** — 그 id는 구성 조회와 편집 응답이
  싣고(`list_configurations`·`get_configuration`의 `id`, 편집 응답의 `configuration_id`)
  `configuration` 인자가 UUID를 받으므로, 실행 시점에 적어 두면 개명 뒤에도 그 구성을 지목할
  수 있다
- **삭제는 살아 있는 세션·페인을 정리하지 않는다.** 프로세스는 계속 돌고 페인은 열려 있고,
  그 세션의 `rerun_session`은 `Configuration '<세션 객체가 만들어질 때의 이름>' not found`로 계속
  실패한다(재실행은 그 이름을 갱신하지 않으므로 마지막 실행 시점의 이름과도 다를 수 있다).
  세션이 구성을 id로 잇으므로 같은 이름의 구성을 새로 만들어도 그 세션의 표적이 되지는
  않는다. 같은 **id**로 되돌아오지 않는 한 계속 실패하며, 그 되돌림은 실재한다 — 지우기 전에
  내보낸 파일을 다시 Import하면 id까지 돌아와 그 세션의 재실행이 다시 이어진다(Import는 id로
  병합한다). 그 파일이 없으면 세션을 버리고 새 구성을 `run_configuration`으로 시작한다.
  응답의 `referencing_compounds`는 compound 참조만 싣는다(4장).
  앱의 Delete 버튼도 이 사실을 경고하지 않는다
- **저장 실패 뒤 같은 `request_id`의 재시도는 흡수된다.** 편집은 메모리에 적용된 시점에
  창에 기록되고 저장은 그 뒤에 비동기로 일어난다. 그래서 "적용됐지만 디스크에 못 썼다"
  상태에서 같은 요청을 다시 보내면 `deduplicated: true` 성공을 받고, 호출자는 영속화됐다고
  결론한다. 신호는 앱 상태바의 `[MCP] Failed to save configurations: …` 한 줄이고, 회복은
  사용자가 앱에서 Save를 누르는 것이다 — 저장 실패가 보이면 다른 `request_id`로 다시 보낸다
- **감사 줄은 한 줄뿐이라 밀려난다.** 2장의 `[MCP]` 표시는 마지막 사건만 남으므로, 권한이
  낮은 호출자가 거부되는 호출을 연속으로 보내 방금 남은 거절 문구를 밀어낼 수 있다. 편집의
  결과 자체는 `list_configurations`로 확인할 수 있지만, "무슨 일이 있었는지"의 이력은 없다
