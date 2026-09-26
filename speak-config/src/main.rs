use anyhow::Result;
use serde::{Deserialize, Serialize};
use slint::{Model, SharedString, VecModel};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

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
    #[serde(default = "default_rate")]
    rate: std::num::NonZeroU32,
    voicevox_default_speaker: Option<u32>,
    aivis_default_speaker: Option<u32>,
    #[serde(default)]
    locale: HashMap<String, String>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

struct AppState {
    voicevox_options: Vec<(String, u32)>, // (Display Name, ID)
    aivis_options: Vec<(String, u32)>,
    config: AppConfig,
}

fn default_rate() -> std::num::NonZeroU32 {
    std::num::NonZeroU32::new(185).unwrap()
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

fn load_config() -> Result<AppConfig> {
    let path = get_config_path();
    println!("Loading config from: {:?}", path);

    Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
}

fn save_config_to_file(config: &AppConfig) -> Result<()> {
    let path = get_config_path();
    println!("Saving config to: {:?}", path);

    let mut current = load_config()?;
    current.rate = config.rate;
    current.voicevox_default_speaker = config.voicevox_default_speaker;
    current.aivis_default_speaker = config.aivis_default_speaker;
    for (locale, voice) in &config.locale {
        if let Some(value) = current.locale.get_mut(locale) {
            *value = voice.clone();
        }
    }
    let content = serde_json::to_string_pretty(&current)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, content)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

fn fetch_speakers_blocking(port: u16) -> Option<Vec<SpeakerInfo>> {
    let url = format!("http://localhost:{}/speakers", port);
    // Use blocking client for simplicity in this thread or use runtime
    // Since we are inside Slint callback usually, we might want to spawn a thread or use blocking.
    // Let's use simple blocking reqwest here to keep it simple,
    // though for UI responsiveness async is better.
    // Given the simplicity, blocking might freeze UI for a fraction of a second, which is acceptable for this tool.
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    match client.get(&url).send() {
        Ok(resp) => resp.json::<Vec<SpeakerInfo>>().ok(),
        Err(_) => None,
    }
}

fn parse_voices(output: &str) -> HashMap<String, Vec<String>> {
    let mut voices: HashMap<String, Vec<String>> = HashMap::new();
    for line in output.lines() {
        let Some((prefix, _)) = line.split_once('#') else {
            continue;
        };
        let prefix = prefix.trim();
        let Some(locale) = prefix.split_whitespace().last() else {
            continue;
        };
        let name = prefix[..prefix.len() - locale.len()].trim();
        if !name.is_empty() && locale.contains('_') {
            voices
                .entry(locale.to_string())
                .or_default()
                .push(name.to_string());
        }
    }
    for names in voices.values_mut() {
        names.sort();
        names.dedup();
    }
    voices
}

fn voice_label(name: &str) -> String {
    // Multilingual macOS names use a nested language/region suffix.
    let mut label = String::new();
    let mut rest = name;
    while let Some(start) = rest.find(" (") {
        label.push_str(&rest[..start]);
        let group = &rest[start..];
        let mut depth = 0;
        let mut nested = false;
        let mut end = None;
        for (index, ch) in group.char_indices() {
            if ch == '(' {
                depth += 1;
                nested |= depth > 1;
            }
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end = Some(index + 1);
                    break;
                }
            }
        }
        let Some(end) = end else {
            label.push_str(group);
            return label;
        };
        if !nested {
            label.push_str(&group[..end]);
        }
        rest = &group[end..];
    }
    label.push_str(rest);
    label.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_defaults_and_rejects_invalid_values() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.rate.get(), 185);
        for value in ["0", "-1", "1.5", "null", "\"fast\""] {
            assert!(serde_json::from_str::<AppConfig>(&format!("{{\"rate\":{value}}}")).is_err());
        }
        let config: AppConfig = serde_json::from_str("{\"rate\":250}").unwrap();
        assert_eq!(serde_json::to_value(config).unwrap()["rate"], 250);
    }

    #[test]
    fn labels_hide_language_but_keep_quality_and_identifiers() {
        assert_eq!(voice_label("Eddy (Korean (South Korea))"), "Eddy");
        assert_eq!(voice_label("Flo (English (UK))"), "Flo");
        assert_eq!(voice_label("Yuna (Premium)"), "Yuna (Premium)");
        assert_eq!(voice_label("Nathan (Enhanced)"), "Nathan (Enhanced)");
    }

    #[test]
    fn parses_names_with_spaces_and_ignores_sample_text() {
        let voices = parse_voices("  Zoe (Premium)       en_US    # Hello en_AU\nJamie (Premium) en_GB # Hello\nYuna ko_KR # Sample\ninvalid\n");
        assert_eq!(voices["en_US"], ["Zoe (Premium)"]);
        assert_eq!(voices["en_GB"], ["Jamie (Premium)"]);
        assert_eq!(voices["ko_KR"], ["Yuna"]);
        assert!(!voices.contains_key("en_AU"));
    }

    #[test]
    fn roundtrip_preserves_custom_locales_and_unknown_settings() {
        let value =
            serde_json::json!({"locale":{"custom":"Custom Voice"}, "other":{"enabled":true}});
        let mut config: AppConfig = serde_json::from_value(value).unwrap();
        config
            .locale
            .insert("custom".into(), "Changed Voice".into());
        let saved = serde_json::to_value(config).unwrap();
        assert_eq!(saved["locale"]["custom"], "Changed Voice");
        assert_eq!(saved["other"]["enabled"], true);
    }
}

fn refresh_voices(window: &AppWindow, state: &Arc<Mutex<AppState>>) {
    let output = std::process::Command::new("say").args(["-v", "?"]).output();
    let voices = match output {
        Ok(output) if output.status.success() => {
            parse_voices(&String::from_utf8_lossy(&output.stdout))
        }
        result => {
            window.set_status_message(format!("Cannot list macOS voices: {result:?}").into());
            HashMap::new()
        }
    };
    let state = state.lock().unwrap();
    window.set_rate_text(state.config.rate.to_string().into());
    let mut locales: Vec<_> = state.config.locale.iter().collect();
    locales.sort_by_key(|(locale, _)| *locale);
    let rows = locales
        .into_iter()
        .map(|(locale, current)| {
            // macOS reports British English as en_GB; retain the user's en_UK key.
            let system_locale = if locale == "en_UK" {
                "en_GB"
            } else {
                locale.as_str()
            };
            let mut names = voices.get(system_locale).cloned().unwrap_or_default();
            if !names.contains(current) {
                names.push(current.clone());
            }
            let selected = names.iter().position(|name| name == current).unwrap_or(0) as i32;
            VoiceRow {
                locale: locale.into(),
                identifiers: Rc::new(VecModel::from(
                    names
                        .iter()
                        .map(|name| SharedString::from(name.as_str()))
                        .collect::<Vec<_>>(),
                ))
                .into(),
                voices: Rc::new(VecModel::from(
                    names
                        .into_iter()
                        .map(|name| SharedString::from(voice_label(&name)))
                        .collect::<Vec<_>>(),
                ))
                .into(),
                selected,
            }
        })
        .collect::<Vec<_>>();
    window.set_voice_rows(Rc::new(VecModel::from(rows)).into());
}

fn main() -> Result<()> {
    let main_window = AppWindow::new()?;
    let state = Arc::new(Mutex::new(AppState {
        voicevox_options: vec![],
        aivis_options: vec![],
        config: load_config()?,
    }));

    let main_window_weak = main_window.as_weak();
    let state_weak = state.clone();

    // Initial Load
    refresh_speakers(&main_window, &state);
    refresh_voices(&main_window, &state);

    let weak = main_window.as_weak();
    let voice_state = state.clone();
    main_window.on_voice_selected(move |row_index, selected| {
        let window = weak.unwrap();
        let model = window.get_voice_rows();
        if let Some(mut row) = model.row_data(row_index as usize) {
            if let Some(voice) = row.identifiers.row_data(selected as usize) {
                voice_state
                    .lock()
                    .unwrap()
                    .config
                    .locale
                    .insert(row.locale.to_string(), voice.to_string());
                row.selected = selected;
                model.set_row_data(row_index as usize, row);
            }
        }
    });

    main_window.on_refresh_speakers(move || {
        let main_window = main_window_weak.unwrap();
        let state = state_weak.clone();
        match load_config() {
            Ok(config) => state.lock().unwrap().config = config,
            Err(error) => {
                main_window.set_status_message(format!("Cannot read settings: {error}").into());
                return;
            }
        }
        refresh_speakers(&main_window, &state);
        refresh_voices(&main_window, &state);
    });

    let main_window_weak = main_window.as_weak();
    let state_weak = state.clone();
    main_window.on_save_config(move |vv_idx, aivis_idx| {
        let main_window = main_window_weak.unwrap();
        let mut state = state_weak.lock().unwrap();

        let Ok(rate) = main_window
            .get_rate_text()
            .trim()
            .parse::<std::num::NonZeroU32>()
        else {
            main_window.set_status_message("Rate must be a positive whole number.".into());
            return;
        };
        state.config.rate = rate;

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

    window.set_status_message("".into());
}
