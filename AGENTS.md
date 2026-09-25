# speak-mcp
Rust 2021 기반의 로컬 텍스트 음성 합성 MCP 서버이며, 표준 입출력으로 MCP 클라이언트와 통신합니다.
`src/main.rs`가 서버 진입점이고 macOS `say`, VOICEVOX(50021), Aivis Speech(10101) 엔진을 지원합니다.
`speak-config/`는 기본 화자 ID를 설정하는 별도 Slint GUI 앱이며, 설정을 사용자 디렉터리에 저장합니다.
설정 스키마 변경 시 `src/main.rs`, `speak-config/`, `install.sh`를 함께 갱신하며 `install_ja.sh`는 이번 범위에서 제외합니다.
