use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const WINDOWS_TRANSPORT_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportRequest {
    pub protocol_version: u32,
    pub operation: WindowsTransportOperation,
    pub session_id: String,
    pub key_event: Option<WindowsTransportKeyEvent>,
    pub candidate_index: Option<usize>,
    pub context: Option<WindowsTransportTextContext>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsTransportOperation {
    KeyEvent,
    Snapshot,
    Commit,
    StopComposition,
    SelectCandidate,
    SubmitSelectedCandidate,
    CloseSession,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WindowsTransportInputLanguage {
    Japanese,
    English,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WindowsTransportInputStyle {
    #[serde(rename = "direct")]
    Direct,
    #[serde(rename = "roman2kana")]
    Roman2Kana,
    #[serde(rename = "defaultRomanToKana")]
    DefaultRomanToKana,
    #[serde(rename = "defaultAZIK")]
    DefaultAzik,
    #[serde(rename = "defaultKanaUS")]
    DefaultKanaUs,
    #[serde(rename = "defaultKanaJIS")]
    DefaultKanaJis,
    #[serde(rename = "empty")]
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportTextContext {
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportKeyEvent {
    pub event_id: u64,
    pub core_key_code: u16,
    pub characters: Option<String>,
    pub characters_ignoring_modifiers: Option<String>,
    pub modifier_flags: i32,
    pub input_style: WindowsTransportInputStyle,
    pub input_language: WindowsTransportInputLanguage,
    pub activate: bool,
    pub live_conversion_enabled: bool,
    pub enable_debug_window: bool,
    pub enable_suggestion: bool,
    pub enable_predictive_typing: bool,
    pub enable_typo_correction: bool,
    pub enable_option_direct_full_width_input: bool,
    pub type_back_slash: bool,
    pub option_direct_input_text: Option<String>,
    pub visible_candidate_start_index: i32,
    pub context: WindowsTransportTextContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportResponse {
    pub protocol_version: u32,
    pub handled: bool,
    pub input_state: WindowsTransportState,
    pub input_language: Option<WindowsTransportInputLanguage>,
    pub effects: Vec<WindowsTransportEffect>,
    pub marked_text: WindowsTransportMarkedText,
    pub candidate_window: WindowsTransportCandidateWindow,
    pub prediction_candidates: Vec<WindowsTransportPredictionCandidate>,
    pub is_empty: bool,
    pub convert_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportState {
    pub kind: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportEffect {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
    pub secondary_text: Option<String>,
    pub input_language: Option<WindowsTransportInputLanguage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportMarkedText {
    pub elements: Vec<WindowsTransportMarkedTextElement>,
    pub selection_location: i32,
    pub selection_length: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowsTransportMarkedTextElement {
    pub content: String,
    pub focus: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportCandidateWindow {
    pub kind: String,
    pub candidates: Vec<WindowsTransportCandidate>,
    pub selection_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportCandidate {
    pub text: String,
    pub annotation_text: Option<String>,
    pub extra_values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTransportPredictionCandidate {
    pub display_text: String,
    pub append_text: String,
    pub delete_count: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_json_uses_stable_cross_language_names() {
        let request = WindowsTransportRequest {
            protocol_version: WINDOWS_TRANSPORT_PROTOCOL_VERSION,
            operation: WindowsTransportOperation::KeyEvent,
            session_id: "win-42-1".into(),
            key_event: Some(WindowsTransportKeyEvent {
                event_id: 7,
                core_key_code: 0,
                characters: Some("k".into()),
                characters_ignoring_modifiers: Some("k".into()),
                modifier_flags: 0,
                input_style: WindowsTransportInputStyle::DefaultRomanToKana,
                input_language: WindowsTransportInputLanguage::Japanese,
                activate: true,
                live_conversion_enabled: true,
                enable_debug_window: false,
                enable_suggestion: true,
                enable_predictive_typing: false,
                enable_typo_correction: false,
                enable_option_direct_full_width_input: false,
                type_back_slash: false,
                option_direct_input_text: None,
                visible_candidate_start_index: 0,
                context: WindowsTransportTextContext::default(),
            }),
            candidate_index: None,
            context: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""protocolVersion":1"#));
        assert!(json.contains(r#""operation":"keyEvent""#));
        assert!(json.contains(r#""sessionId":"win-42-1""#));
        assert!(json.contains(r#""inputStyle":"defaultRomanToKana""#));
        assert!(json.contains(r#""inputLanguage":"japanese""#));
        assert!(json.contains(r#""charactersIgnoringModifiers":"k""#));
    }

    #[test]
    fn response_json_decodes_candidate_snapshot() {
        let json = r#"{
            "protocolVersion":1,
            "handled":true,
            "inputState":{"kind":"composing","value":null},
            "inputLanguage":"japanese",
            "effects":[],
            "markedText":{
                "elements":[{"content":"変換","focus":"focused"}],
                "selectionLocation":2,
                "selectionLength":0
            },
            "candidateWindow":{
                "kind":"composing",
                "candidates":[
                    {"text":"変換","annotationText":null,"extraValues":{}}
                ],
                "selectionIndex":0
            },
            "predictionCandidates":[],
            "isEmpty":false,
            "convertTarget":"へんかん"
        }"#;

        let response: WindowsTransportResponse = serde_json::from_str(json).unwrap();
        assert!(response.handled);
        assert_eq!(response.convert_target, "へんかん");
        assert_eq!(response.candidate_window.candidates[0].text, "変換");
    }
}
