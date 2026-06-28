use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: ModelMessage,
    pub finish_reason: FinishReason,
    pub usage: UsageInfo,
    pub model_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: String,
    pub provider: String,
    pub model_name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub cost_per_1m_input: f64,
    pub cost_per_1m_output: f64,
    pub suitable_for: Vec<String>,
}

/// A single event from a streaming chat response.
/// Mirrors the OpenAI SSE stream format (text deltas, tool calls, usage).
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Incremental text content delta.
    TextDelta(String),
    /// Tool call fragment — accumulated across multiple deltas.
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    /// Final usage information (sent once at stream end).
    Usage(UsageInfo),
    /// Stream complete.
    Done,
    /// Stream encountered an error.
    Error(String),
}

/// A handle to consume streaming responses via a channel.
/// Clones share the same underlying receiver.
#[derive(Debug, Clone)]
pub struct StreamHandle {
    rx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>>>>,
}

impl StreamHandle {
    pub(crate) fn new(rx: tokio::sync::mpsc::UnboundedReceiver<StreamEvent>) -> Self {
        Self {
            rx: Arc::new(std::sync::Mutex::new(Some(rx))),
        }
    }

    /// Receive the next stream event. Returns `None` when the stream is exhausted.
    pub async fn recv(&self) -> Option<StreamEvent> {
        let mut rx = self.rx.lock().unwrap().take()?;
        let event = rx.recv().await;
        self.rx.lock().unwrap().replace(rx);
        event
    }

    /// Collect all remaining events into a vector.
    pub async fn collect(&self) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.recv().await {
            match event {
                StreamEvent::Done => break,
                StreamEvent::Error(_) => {
                    events.push(event);
                    break;
                }
                _ => events.push(event),
            }
        }
        events
    }
}

/// A model family (e.g. "Gemini 2.0") containing specific variants (Flash, Pro, …).
#[derive(Debug, Clone)]
pub struct ModelFamily {
    pub name: String,
    pub variants: Vec<ModelProfile>,
}

/// Group models into families using a heuristic that strips trailing variant/version
/// segments from model IDs. Models that share a common stem are grouped together.
///
/// Stems are derived by removing known variant suffixes (`mini`, `flash`, `pro`, `turbo`,
/// version numbers, date stamps, etc.) and checking what remains.
pub fn group_models_by_family(models: Vec<ModelProfile>) -> Vec<ModelFamily> {
    use std::collections::HashMap;

    let mut families: HashMap<String, Vec<ModelProfile>> = HashMap::new();

    for model in models {
        let stem = extract_model_stem(&model.id);
        let label = stem
            .split('/')
            .last()
            .unwrap_or(&stem)
            .replace('-', " ")
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ");

        families.entry(label).or_default().push(model);
    }

    let mut result: Vec<ModelFamily> = families
        .into_iter()
        .map(|(name, variants)| ModelFamily { name, variants })
        .collect();

    // Sort: Berdasarkan skor prioritas NVIDIA (descending), lalu alfabetis jika skor sama
    let get_priority_score = |name: &str| -> i32 {
        let name_lower = name.to_lowercase();
        if name_lower.contains("minimax-m3") { return 1000; }
        if name_lower.contains("diffusiongemma") { return 990; }
        if name_lower.contains("nemotron-3-ultra") { return 980; }
        if name_lower.contains("nemotron-3.5-content-safety") { return 970; }
        if name_lower.contains("cosmos3-nano-reasoner") { return 960; }
        if name_lower.contains("cosmos3-nano") { return 950; }
        if name_lower.contains("step-3.7") { return 940; }
        if name_lower.contains("kimi-k2.6") { return 930; }
        if name_lower.contains("mistral-medium") { return 920; }
        if name_lower.contains("nemotron-3-nano-omni") { return 910; }
        if name_lower.contains("deepseek-v4-flash") { return 900; }
        if name_lower.contains("deepseek-v4-pro") { return 890; }
        if name_lower.contains("glm-5.1") { return 880; }
        if name_lower.contains("nemotron-3-content-safety") { return 870; }
        if name_lower.contains("synthetic-video-detector") { return 860; }
        if name_lower.contains("active speaker") || name_lower.contains("active-speaker") { return 850; }
        if name_lower.contains("ising-calibration") { return 840; }
        if name_lower.contains("minimax-m2.7") { return 830; }
        if name_lower.contains("gemma-4-31b") { return 820; }
        if name_lower.contains("mistral-small-4") { return 810; }
        if name_lower.contains("nemotron-voicechat") { return 800; }
        if name_lower.contains("nemotron-3-super") { return 790; }
        if name_lower.contains("qwen3.5-122b") { return 780; }
        if name_lower.contains("gliner-pii") { return 770; }
        if name_lower.contains("cosmos-transfer2.5") { return 760; }
        if name_lower.contains("qwen3.5-397b") { return 750; }
        if name_lower.contains("step-3.5") { return 740; }
        if name_lower.contains("nemotron-content-safety-reasoning") { return 730; }
        if name_lower.contains("nemotron-3-nano-30b") { return 720; }
        if name_lower.contains("riva-translate") { return 710; }
        if name_lower.contains("mistral-large-3") { return 700; }
        if name_lower.contains("ministral-14b") { return 690; }
        if name_lower.contains("streampetr") { return 680; }
        if name_lower.contains("nemotron-nano-12b") { return 670; }
        if name_lower.contains("llama-3.1-nemotron-safety-guard") { return 660; }
        if name_lower.contains("stockmark-2-100b") { return 650; }
        if name_lower.contains("qwen3-next-80b") { return 640; }
        if name_lower.contains("seed-oss-36b") { return 630; }
        if name_lower.contains("nvidia-nemotron-nano-9b") { return 620; }
        if name_lower.contains("gpt-oss-20b") { return 610; }
        if name_lower.contains("gpt-oss-120b") { return 600; }
        if name_lower.contains("llama-3.3-nemotron-super") { return 590; }
        if name_lower.contains("sarvam-m") { return 580; }
        if name_lower.contains("llama-guard-4-12b") { return 570; }
        if name_lower.contains("gemma-3n-e4b") { return 560; }
        if name_lower.contains("gemma-3n-e2b") { return 550; }
        if name_lower.contains("cosmos-transfer1") { return 540; }
        if name_lower.contains("background noise") || name_lower.contains("background-noise") { return 530; }
        if name_lower.contains("mistral-nemotron") { return 520; }
        if name_lower.contains("llama-3.1-nemotron-nano-vl") { return 510; }
        if name_lower.contains("magpie-tts") { return 500; }
        if name_lower.contains("llama-4-maverick") { return 490; }
        if name_lower.contains("sparsedrive") { return 480; }
        if name_lower.contains("bevformer") { return 470; }
        if name_lower.contains("llama-3.1-nemotron-nano") { return 460; }
        if name_lower.contains("nv-embedcode") { return 450; }
        if name_lower.contains("phi-4-mini") { return 440; }
        if name_lower.contains("phi-4-multimodal") { return 430; }
        if name_lower.contains("llama-3.3-70b") { return 420; }
        if name_lower.contains("studio voice") || name_lower.contains("studio-voice") { return 410; }
        if name_lower.contains("llama-3.2-3b") { return 400; }
        if name_lower.contains("llama-3.2-11b") { return 390; }
        if name_lower.contains("llama-3.2-90b") { return 380; }
        if name_lower.contains("llama-3.2-1b") { return 370; }
        if name_lower.contains("dracarys-llama") { return 360; }
        if name_lower.contains("esm2-650m") { return 350; }
        if name_lower.contains("nemotron-mini-4b") { return 340; }
        if name_lower.contains("gemma-2-2b") { return 330; }
        if name_lower.contains("llama-3.1-70b") { return 320; }
        if name_lower.contains("llama-3.1-8b") { return 310; }
        if name_lower.contains("nv-embed-v1") { return 300; }
        if name_lower.contains("solar-10.7b") { return 290; }
        if name_lower.contains("paligemma") { return 280; }
        if name_lower.contains("rerank-qa") { return 270; }
        if name_lower.contains("esmfold") { return 260; }

        // Keyword umum jika tidak cocok spesifik
        if name_lower.contains("deepseek") { return 100; }
        if name_lower.contains("kimi") { return 90; }
        if name_lower.contains("llama") || name_lower.contains("nemotron") { return 80; }
        if name_lower.contains("mistral") || name_lower.contains("codestral") { return 70; }
        if name_lower.contains("qwen") { return 60; }
        if name_lower.contains("gemma") { return 50; }
        if name_lower.contains("phi") { return 20; }
        0
    };

    result.sort_by(|a, b| {
        let a_score = get_priority_score(&a.name);
        let b_score = get_priority_score(&b.name);
        if a_score != b_score {
            b_score.cmp(&a_score) // Descending score
        } else {
            a.name.cmp(&b.name) // Alfabetis jika score sama
        }
    });
    result
}

/// Remove trailing variant/version segments from a model ID to find its stem.
///
/// Examples:
/// - `gemini-2.0-flash`       → `gemini-2.0`
/// - `gemini-2.0-flash-lite`  → `gemini-2.0`
/// - `gpt-4o-mini`            → `gpt-4o`
/// - `claude-sonnet-4-20250514` → `claude-sonnet-4`
fn extract_model_stem(model_id: &str) -> String {
    let id = model_id.split('/').last().unwrap_or(model_id);

    let variant_suffixes = [
        "mini", "micro", "nano", "small", "medium", "large",
        "flash", "pro", "turbo", "lite", "ultra", "max", "plus",
        "sonnet", "haiku", "opus",
        "instruct", "chat", "code", "reasoning", "thinking",
        "preview", "exp", "experimental", "latest", "stable",
        "vision", "audio", "realtime",
    ];

    let parts: Vec<&str> = id.split('-').collect();
    if parts.len() <= 2 {
        return id.to_string();
    }

    // Walk backwards and strip known variant/version segments
    let mut end = parts.len();
    for i in (1..parts.len()).rev() {
        let segment = parts[i];
        // Version number or date
        if segment.chars().all(|c| c.is_ascii_digit() || c == '.') {
            end = i;
            continue;
        }
        // Known variant suffix
        if variant_suffixes.contains(&segment) {
            end = i;
            continue;
        }
        break;
    }

    parts[..end].join("-")
}
