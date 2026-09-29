use serde::{Deserialize, Serialize};

pub const WINDOWS_BRIDGE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsBridgeOperation {
    KeyEvent,
    Snapshot,
    Commit,
    StopComposition,
    CloseSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsBridgeLanguage {
    Japanese,
    English,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsBridgeInputStyle {
    Direct,
    Roman2kana,
    DefaultRomanToKana,
    DefaultAzik,
    DefaultKanaUs,
    DefaultKanaJis,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeTextContext {
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeKeyEvent {
    pub event_id: u64,
    pub modifier_flags: i32,
    pub characters: Option<String>,
    pub characters_ignoring_modifiers: Option<String>,
    pub key_code: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeRequest {
    pub windows_bridge_version: u32,
    pub operation: WindowsBridgeOperation,
    pub session_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub activate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<WindowsBridgeLanguage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_style: Option<WindowsBridgeInputStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_conversion_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_suggestion: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_predictive_typing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_typo_correction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_back_slash: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_candidate_start_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<WindowsBridgeKeyEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<WindowsBridgeTextContext>,
}

impl WindowsBridgeRequest {
    pub fn key_event(
        session_id: impl Into<String>,
        event: WindowsBridgeKeyEvent,
        activate: bool,
    ) -> Self {
        Self {
            windows_bridge_version: WINDOWS_BRIDGE_VERSION,
            operation: WindowsBridgeOperation::KeyEvent,
            session_id: session_id.into(),
            activate: Some(activate),
            language: Some(WindowsBridgeLanguage::Japanese),
            input_style: Some(WindowsBridgeInputStyle::DefaultRomanToKana),
            live_conversion_enabled: Some(true),
            enable_suggestion: Some(true),
            enable_predictive_typing: Some(false),
            enable_typo_correction: Some(false),
            type_back_slash: Some(false),
            visible_candidate_start_index: Some(0),
            event: Some(event),
            context: Some(WindowsBridgeTextContext::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeMarkedTextElement {
    pub text: String,
    pub focus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeCandidate {
    pub text: String,
    pub annotation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeCandidateWindow {
    pub state: String,
    pub candidates: Vec<WindowsBridgeCandidate>,
    pub selection_index: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeEffect {
    pub kind: String,
    pub text: Option<String>,
    pub secondary_text: Option<String>,
    pub language: Option<WindowsBridgeLanguage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsBridgeResponse {
    pub windows_bridge_version: u32,
    pub handled: bool,
    pub input_state: String,
    pub input_state_value: Option<String>,
    pub input_language: Option<WindowsBridgeLanguage>,
    pub marked_text: Vec<WindowsBridgeMarkedTextElement>,
    pub selection_location: i64,
    pub selection_length: i64,
    pub candidate_window: WindowsBridgeCandidateWindow,
    pub effects: Vec<WindowsBridgeEffect>,
    pub is_empty: bool,
    pub convert_target: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_key_event_with_stable_field_names() {
        let request = WindowsBridgeRequest::key_event(
            "test-session",
            WindowsBridgeKeyEvent {
                event_id: 7,
                modifier_flags: 0,
                characters: Some("a".into()),
                characters_ignoring_modifiers: Some("a".into()),
                key_code: 0,
            },
            true,
        );

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""windowsBridgeVersion":1"#));
        assert!(json.contains(r#""operation":"keyEvent""#));
        assert!(json.contains(r#""sessionID":"test-session""#));
        assert!(json.contains(r#""eventID":7"#));
        assert!(json.contains(r#""inputStyle":"defaultRomanToKana""#));
    }

    #[test]
    fn decodes_minimal_bridge_response() {
        let json = r#"{
            "windowsBridgeVersion":1,
            "handled":true,
            "inputState":"composing",
            "inputStateValue":null,
            "inputLanguage":"japanese",
            "markedText":[{"text":"あ","focus":"focused"}],
            "selectionLocation":1,
            "selectionLength":0,
            "candidateWindow":{"state":"hidden","candidates":[],"selectionIndex":null},
            "effects":[],
            "isEmpty":false,
            "convertTarget":"あ"
        }"#;

        let response: WindowsBridgeResponse = serde_json::from_str(json).unwrap();
        assert!(response.handled);
        assert_eq!(response.convert_target, "あ");
        assert_eq!(response.marked_text[0].text, "あ");
    }
}
