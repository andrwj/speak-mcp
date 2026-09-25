use anyhow::Result;
use serde::{Deserialize, Serialize};
use slint::{Model, SharedString, VecModel};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

slint::include_modules!();

#[derive(Debug, Deserialize)]
struct StyleInfo {
    name: String,
    id: u32,
}

#[derive(Debug, Deserialize)]
struct SpeakerInfo {
    name: String,
    styles: Vec<StyleInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct AppConfig {
    voicevox_default_speaker: Option<u32>,
    aivis_default_speaker: Option<u32>,
    #[serde(rename = "en_US", default = "default_en_us_voice")]
    en_us: String,
    #[serde(rename = "en_AU", default = "default_en_au_voice")]
    en_au: String,
    #[serde(rename = "en_UK", default = "default_en_uk_voice")]
    en_uk: String,
    #[serde(rename = "ko_KR", default = "default_ko_kr_voice")]
    ko_kr: String,
}

fn default_en_us_voice() -> String {
    "Nathan (Enhanced)".to_string()
}

fn default_en_au_voice() -> String {
    "Karen (Premium)".to_string()
}

fn default_en_uk_voice() -> String {
    "Jamie (Enhanced)".to_string()
}

fn default_ko_kr_voice() -> String {
    "Yuna (Premium)".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            voicevox_default_speaker: None,
            aivis_default_speaker: None,
            en_us: default_en_us_voice(),
            en_au: default_en_au_voice(),
            en_uk: default_en_uk_voice(),
            ko_kr: default_ko_kr_voice(),
        }
    }
}

struct AppState {
    voicevox_options: Vec<(String, u32)>, // (Display Name, ID)
    aivis_options: Vec<(String, u32)>,
    config: AppConfig,
}

fn get_config_path() -> PathBuf {
    if let Some(mut home) = dirs::home_dir() {
        home.push(".config");
        home.push("speak-mcp");
        if !home.exists() {
            let _ = std::fs::create_dir_all(&home);
        }
        home.push("config.json");
        return home;
    }

    PathBuf::from(".config/speak-mcp/config.json")
}

fn load_config() -> AppConfig {
    let path = get_config_path();
    println!("Loading config from: {:?}", path);

    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str(&content) {
            println!("Config loaded: {:?}", config);
            return config;
        }
    } else {
        println!("Config file not found or unreadable at {:?}", path);
    }
    println!("Using default config");
    AppConfig::default()
}

fn save_config_to_file(config: &AppConfig) -> Result<()> {
    let path = get_config_path();
    println!("Saving config to: {:?}", path);

    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

fn fetch_speakers_blocking(port: u16) -> Option<Vec<SpeakerInfo>> {
    let url = format!("http://localhost:{}/speakers", port);
    // Use blocking client for simplicity in this thread or use runtime
    // Since we are inside Slint callback usually, we might want to spawn a thread or use blocking.
    // Let's use simple blocking reqwest here to keep it simple,
    // though for UI responsiveness async is better.
    // Given the simplicity, blocking might freeze UI for a fraction of a second, which is acceptable for this tool.
    match reqwest::blocking::get(&url) {
        Ok(resp) => resp.json::<Vec<SpeakerInfo>>().ok(),
        Err(_) => None,
    }
}

fn main() -> Result<()> {
    let main_window = AppWindow::new()?;
    let state = Arc::new(Mutex::new(AppState {
        voicevox_options: vec![],
        aivis_options: vec![],
        config: load_config(),
    }));

    let main_window_weak = main_window.as_weak();
    let state_weak = state.clone();

    // Initial Load
    refresh_speakers(&main_window, &state);

    main_window.on_refresh_speakers(move || {
        let main_window = main_window_weak.unwrap();
        let state = state_weak.clone();
        refresh_speakers(&main_window, &state);
    });

    let main_window_weak = main_window.as_weak();
    let state_weak = state.clone();
    main_window.on_save_config(move |vv_idx, aivis_idx| {
        let main_window = main_window_weak.unwrap();
        let mut state = state_weak.lock().unwrap();

        let vv_id = if vv_idx >= 0 && (vv_idx as usize) < state.voicevox_options.len() {
            Some(state.voicevox_options[vv_idx as usize].1)
        } else {
            None
        };

        let aivis_id = if aivis_idx >= 0 && (aivis_idx as usize) < state.aivis_options.len() {
            Some(state.aivis_options[aivis_idx as usize].1)
        } else {
            None
        };

        state.config.voicevox_default_speaker = vv_id;
        state.config.aivis_default_speaker = aivis_id;

        match save_config_to_file(&state.config) {
            Ok(_) => main_window.set_status_message("Settings saved successfully!".into()),
            Err(e) => main_window.set_status_message(format!("Error saving: {}", e).into()),
        }
    });

    main_window.run()?;
    Ok(())
}

fn refresh_speakers(window: &AppWindow, state: &Arc<Mutex<AppState>>) {
    let mut state = state.lock().unwrap();
    window.set_status_message("Fetching speakers...".into());

    // Fetch VOICEVOX
    let mut vv_list = Vec::new();
    let mut vv_options = Vec::new();
    let mut vv_default_idx = 0;

    // Add "Default/Auto" option
    vv_list.push(SharedString::from("Default / Auto (ID: 1)"));
    vv_options.push(("Default".to_string(), 1));

    if let Some(speakers) = fetch_speakers_blocking(50021) {
        for speaker in speakers {
            for style in speaker.styles {
                let label = format!("{} ({})", speaker.name, style.name);
                vv_list.push(SharedString::from(&label));
                vv_options.push((label, style.id));

                if Some(style.id) == state.config.voicevox_default_speaker {
                    vv_default_idx = vv_options.len() as i32 - 1;
                }
            }
        }
    }
    state.voicevox_options = vv_options;
    let vv_model = Rc::new(VecModel::from(vv_list));
    window.set_voicevox_model(vv_model.into());
    window.set_voicevox_index(vv_default_idx);

    // Fetch Aivis
    let mut aivis_list = Vec::new();
    let mut aivis_options = Vec::new();
    let mut aivis_default_idx = 0;

    aivis_list.push(SharedString::from("Default / Auto (ID: 1)"));
    aivis_options.push(("Default".to_string(), 1));

    if let Some(speakers) = fetch_speakers_blocking(10101) {
        for speaker in speakers {
            for style in speaker.styles {
                let label = format!("{} ({})", speaker.name, style.name);
                aivis_list.push(SharedString::from(&label));
                aivis_options.push((label, style.id));

                if Some(style.id) == state.config.aivis_default_speaker {
                    aivis_default_idx = aivis_options.len() as i32 - 1;
                }
            }
        }
    }
    state.aivis_options = aivis_options;
    let aivis_model = Rc::new(VecModel::from(aivis_list));
    window.set_aivis_model(aivis_model.into());
    window.set_aivis_index(aivis_default_idx);

    window.set_status_message("Ready".into());
}
