use anyhow::Result;
use async_mcp::server::Server;
use async_mcp::types::{
    CallToolRequest, CallToolResponse, ListRequest, ServerCapabilities, Tool, ToolResponseContent,
    ToolsListResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Write;
use std::process::Command;
use std::{collections::HashMap, sync::Arc};

#[cfg(target_os = "macos")]
mod speech_queue;
mod stdio;

#[derive(Debug, Deserialize, Serialize)]
struct SpeakArgs {
    text: String,
    locale: String,
    speed: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct VoiceEngineArgs {
    text: String,
    speaker: Option<u32>,
    speed: Option<f32>,
}

#[derive(Debug, Deserialize, Clone)]
struct StyleInfo {
    name: String,
    id: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct SpeakerInfo {
    name: String,
    styles: Vec<StyleInfo>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct AppConfig {
    voicevox_default_speaker: Option<u32>,
    aivis_default_speaker: Option<u32>,
    #[serde(default)]
    locale: HashMap<String, String>,
}

impl AppConfig {
    fn voice_for_locale(&self, locale: &str) -> Option<&str> {
        self.locale.get(locale).map(String::as_str)
    }
}

fn get_config_path() -> std::path::PathBuf {
    if let Some(mut home) = dirs::home_dir() {
        home.push(".config");
        home.push("speak-mcp");
        home.push("config.json");
        return home;
    }

    std::path::PathBuf::from(".config/speak-mcp/config.json")
}

fn load_config() -> AppConfig {
    let path = get_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str(&content) {
            return config;
        }
    }

    AppConfig::default()
}

async fn fetch_speakers(port: u16) -> Option<Vec<SpeakerInfo>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .unwrap_or_default();
    let url = format!("http://localhost:{}/speakers", port);
    match client.get(&url).send().await {
        Ok(resp) => resp.json::<Vec<SpeakerInfo>>().await.ok(),
        Err(_) => None,
    }
}

fn build_speaker_choice_schema(
    speakers: Option<Vec<SpeakerInfo>>,
    default_id: Option<u32>,
) -> serde_json::Value {
    // Default to 1 if no config and no speakers found, but if config exists use it.
    let default_val = default_id.unwrap_or(1);

    if let Some(speakers) = speakers {
        let mut one_of = Vec::new();

        for speaker in speakers {
            for style in speaker.styles {
                one_of.push(json!({
                    "const": style.id,
                    "title": format!("{} ({})", speaker.name, style.name)
                }));
            }
        }

        // Ensure default value is in the list if possible, or add a fallback option
        // In a perfect world we check validation, but for now we trust the config or list.

        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "speaker": {
                    "oneOf": one_of,
                    "default": default_val
                },
                "speed": { "type": "number", "default": 1.0 }
            },
            "required": ["text"]
        })
    } else {
        // Fallback schema if engine is offline
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "speaker": { "type": "integer", "default": default_val },
                "speed": { "type": "number", "default": 1.0 }
            },
            "required": ["text"]
        })
    }
}

async fn play_wav(wav_data: &[u8]) -> Result<()> {
    let mut temp_file = tempfile::NamedTempFile::new()?;
    temp_file.write_all(wav_data)?;
    let path = temp_file
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid path"))?;

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("afplay").arg(path).output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!("afplay failed"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "(New-Object System.Media.SoundPlayer '{}').PlaySync()",
            path
        );
        let output = Command::new("powershell")
            .arg("-Command")
            .arg(script)
            .output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!("PowerShell playback failed"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let script = format!(
            "export XDG_RUNTIME_DIR=/run/user/1000; pw-play '{}' || aplay -q '{}'",
            path, path
        );
        let output = Command::new("sh").arg("-c").arg(script).output()?;
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Linux playback failed: pw-play and aplay both failed."
            ));
        }
    }

    Ok(())
}

async fn call_voicevox_compatible(
    port: u16,
    req: CallToolRequest,
    default_speaker: Option<u32>,
) -> Result<CallToolResponse> {
    let args_map = req
        .arguments
        .ok_or_else(|| anyhow::anyhow!("Arguments missing"))?;
    let args: VoiceEngineArgs = serde_json::from_value(serde_json::to_value(args_map)?)?;

    // Use argument speaker if provided, otherwise config default, otherwise 1
    let speaker_id = args.speaker.or(default_speaker).unwrap_or(1);

    let speed_scale = args.speed.unwrap_or(1.0);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let base_url = format!("http://localhost:{}", port);

    let query_res = client
        .post(format!("{}/audio_query", base_url))
        .query(&[("text", &args.text), ("speaker", &speaker_id.to_string())])
        .send()
        .await?;
    let mut query_json: serde_json::Value = query_res.json().await?;
    query_json["speedScale"] = json!(speed_scale);

    let synthesis_res = client
        .post(format!("{}/synthesis", base_url))
        .query(&[("speaker", &speaker_id.to_string())])
        .json(&query_json)
        .send()
        .await?;
    let wav_data = synthesis_res.bytes().await?;

    play_wav(&wav_data).await?;

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text {
            text: "読み上げ完了！✨".to_string(),
        }],
        is_error: Some(false),
        meta: None,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let transport = stdio::StdioTransport::new();
    let config = load_config();

    // Fetch speakers at startup
    // Note: We intentionally ignore errors here and fallback to default schema
    // to ensure the server starts even if TTS engines are down.
    let voicevox_speakers = fetch_speakers(50021).await;
    let aivis_speakers = fetch_speakers(10101).await;

    // Build tools list
    let vv_default = config.voicevox_default_speaker;
    let aivis_default = config.aivis_default_speaker;

    let mut tools = Vec::new();

    // VOICEVOX tool
    tools.push(Tool {
        name: "speak_voicevox".to_string(),
        description: Some("VOICEVOXを使用して読み上げます。(Port: 50021)".to_string()),
        input_schema: build_speaker_choice_schema(voicevox_speakers.as_ref().cloned(), vv_default),
        output_schema: None,
    });

    // Aivis Speech tool
    tools.push(Tool {
        name: "speak_aivis".to_string(),
        description: Some("Aivis Speechを使用して読み上げます。(Port: 10101)".to_string()),
        input_schema: build_speaker_choice_schema(aivis_speakers.as_ref().cloned(), aivis_default),
        output_schema: None,
    });

    #[cfg(target_os = "macos")]
    {
        tools.push(Tool {
            name: "speak".to_string(),
                description: Some("Use when the user wants to hear a response spoken aloud. `locale` is required; use one of `en_US`, `en_AU`, `en_UK`, or `ko_KR` to match the response language. Queue speech using macOS say; this non-blocking tool returns immediately after acceptance, before playback finishes. For long responses, call the tool with one paragraph or a small group of paragraphs at a time rather than the entire response at once. Jobs play sequentially in FIFO order.".to_string()),
            input_schema: json!({
                "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "locale": { "type": "string" },
                        "speed": { "type": "integer" }
                    },
                    "required": ["text", "locale"]
            }),
            output_schema: None,
        });
    }

    // Share tools and config across handlers
    let tools_arc = Arc::new(tools);
    let config_arc = Arc::new(config);

    #[cfg(target_os = "macos")]
    let (speech_queue, stop_speech, speech_worker) = speech_queue::SpeechQueue::start();

    let builder = Server::builder(transport)
        .name("speak-mcp")
        .version("0.1.0")
        .capabilities(ServerCapabilities {
            tools: Some(json!({})),
            ..Default::default()
        })
        .request_handler("tools/list", {
            let tools = tools_arc.clone();
            // MCP clients may omit params entirely when listing tools.
            move |_req: Option<ListRequest>| {
                let tools = tools.clone();
                Box::pin(async move {
                    Ok(ToolsListResponse {
                        tools: tools.as_ref().clone(),
                        next_cursor: None,
                        meta: None,
                    })
                })
            }
        })
        .request_handler("tools/call", {
            let config = config_arc.clone();
            move |req: CallToolRequest| {
                let config = config.clone();
                #[cfg(target_os = "macos")]
                let speech_queue = speech_queue.clone();
                Box::pin(async move {
                    match req.name.as_str() {
                        "speak_voicevox" => {
                            let default = config.voicevox_default_speaker;
                            call_voicevox_compatible(50021, req, default).await
                        }
                        "speak_aivis" => {
                            let default = config.aivis_default_speaker;
                            call_voicevox_compatible(10101, req, default).await
                        }
                        #[cfg(target_os = "macos")]
                        "speak" => {
                            let args_map = req
                                .arguments
                                .ok_or_else(|| anyhow::anyhow!("Arguments missing"))?;
                            let args: SpeakArgs =
                                serde_json::from_value(serde_json::to_value(args_map)?)?;
                            let id = speech_queue.enqueue(args)?;
                            Ok(CallToolResponse {
                                content: vec![ToolResponseContent::Text {
                                    text: json!({"status": "queued", "job_id": id}).to_string(),
                                }],
                                is_error: Some(false),
                                meta: None,
                            })
                        }
                        _ => Err(anyhow::anyhow!("Unknown tool: {}", req.name)),
                    }
                })
            }
        });

    let server = builder.build();
    eprintln!("Speak MCP Server (Multi-Engine) starting...");
    let result = server.listen().await;
    #[cfg(target_os = "macos")]
    {
        let _ = stop_speech.send(());
        speech_worker.await?;
    }
    result?;

    Ok(())
}
