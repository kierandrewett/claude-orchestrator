use anyhow::Result;
use teloxide::prelude::*;

use claude_orchestrator_llm::{InterpretedVoiceCommand, OrchestratorLlm, VoiceContext};

use crate::files::download_file;

/// Handle a Telegram voice message: download → transcribe → interpret.
pub async fn handle_voice(
    bot: &Bot,
    file_id: &str,
    context: &VoiceContext,
    llm: &OrchestratorLlm,
    stt_api_key: Option<&str>,
) -> Result<InterpretedVoiceCommand> {
    // 1. Download the voice file (.ogg).
    let audio_data = download_file(bot, file_id).await?;

    // 2. Transcribe — use Whisper API if key is provided, else return empty passthrough.
    let transcript = if let Some(api_key) = stt_api_key {
        transcribe_whisper(&audio_data, api_key)
            .await
            .unwrap_or_default()
    } else {
        String::new()
    };

    if transcript.is_empty() {
        return Ok(InterpretedVoiceCommand::Passthrough {
            text: "[voice message — transcription unavailable]".to_string(),
        });
    }

    // 3. Interpret with the orchestrator LLM.
    let cmd = llm.interpret_voice(&transcript, context).await?;
    Ok(cmd)
}

/// Transcribe audio using the OpenAI Whisper API.
async fn transcribe_whisper(audio_data: &[u8], api_key: &str) -> Result<String> {
    use reqwest::multipart;

    let client = reqwest::Client::new();
    let part = multipart::Part::bytes(audio_data.to_vec())
        .file_name("voice.ogg")
        .mime_str("audio/ogg")?;

    let form = multipart::Form::new()
        .text("model", "whisper-1")
        .part("file", part);

    let resp: serde_json::Value = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await?
        .json()
        .await?;

    Ok(resp["text"].as_str().unwrap_or("").to_string())
}
