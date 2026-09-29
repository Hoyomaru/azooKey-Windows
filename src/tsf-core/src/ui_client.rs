use std::{
    io,
    sync::OnceLock,
    time::{Duration, Instant},
};

use hyper_util::rt::TokioIo;
use shared::{
    window::{
        window_service_client::WindowServiceClient, CandidateItem, Empty, SetCandidatesRequest,
        SetPositionRequest, SetSelectionRequest, WindowPosition,
    },
    windows_transport::WindowsTransportResponse,
};
use tokio::{
    net::windows::named_pipe::ClientOptions,
    runtime::Runtime,
    time,
};
use tonic::transport::{Channel, Endpoint};
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

const UI_PIPE_NAME: &str = r"\\.\pipe\azookey-ui";
const DUMMY_URI: &str = "http://[::]:50052";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);
const PIPE_OPEN_TIMEOUT: Duration = Duration::from_millis(75);

static UI_RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    UI_RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create candidate UI runtime"))
}

pub fn publish_response_best_effort(
    response: &WindowsTransportResponse,
    caret_rect: Option<(i32, i32, i32, i32)>,
) {
    let update = CandidateWindowUpdate::from_response(response, caret_rect);
    runtime().spawn(async move {
        if let Err(error) = publish(update).await {
            tracing::debug!("Candidate UI unavailable: {error:?}");
        }
    });
}

pub fn publish_position_best_effort(caret_rect: (i32, i32, i32, i32)) {
    runtime().spawn(async move {
        if let Err(error) = publish_position(caret_rect).await {
            tracing::debug!("Candidate UI position update unavailable: {error:?}");
        }
    });
}

pub fn hide_best_effort() {
    runtime().spawn(async move {
        if let Err(error) = hide().await {
            tracing::debug!("Candidate UI hide unavailable: {error:?}");
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateWindowUpdate {
    visible: bool,
    candidates: Vec<CandidateItemData>,
    selection_index: Option<i32>,
    caret_rect: Option<(i32, i32, i32, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateItemData {
    text: String,
    annotation: String,
}

impl CandidateWindowUpdate {
    fn from_response(
        response: &WindowsTransportResponse,
        caret_rect: Option<(i32, i32, i32, i32)>,
    ) -> Self {
        let visible = response.candidate_window.kind != "hidden"
            && !response.candidate_window.candidates.is_empty();
        let candidates = response
            .candidate_window
            .candidates
            .iter()
            .map(|candidate| CandidateItemData {
                text: candidate.text.clone(),
                annotation: candidate.annotation_text.clone().unwrap_or_default(),
            })
            .collect();
        let selection_index = response
            .candidate_window
            .selection_index
            .and_then(|index| i32::try_from(index).ok());

        Self {
            visible,
            candidates,
            selection_index,
            caret_rect,
        }
    }
}

async fn publish(update: CandidateWindowUpdate) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let channel = connect().await?;
    let mut client = WindowServiceClient::new(channel);

    if !update.visible {
        client.hide(Empty {}).await?;
        return Ok(());
    }

    client
        .set_candidates(SetCandidatesRequest {
            candidates: update
                .candidates
                .into_iter()
                .map(|candidate| CandidateItem {
                    text: candidate.text,
                    annotation: candidate.annotation,
                })
                .collect(),
        })
        .await?;

    if let Some(index) = update.selection_index {
        client
            .set_selection(SetSelectionRequest { index })
            .await?;
    }

    if let Some((top, left, bottom, right)) = update.caret_rect {
        client
            .set_position(SetPositionRequest {
                position: Some(WindowPosition {
                    top,
                    left,
                    bottom,
                    right,
                }),
            })
            .await?;
    }

    client.show(Empty {}).await?;
    Ok(())
}

async fn publish_position(
    caret_rect: (i32, i32, i32, i32),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let channel = connect().await?;
    let mut client = WindowServiceClient::new(channel);
    let (top, left, bottom, right) = caret_rect;
    client
        .set_position(SetPositionRequest {
            position: Some(WindowPosition {
                top,
                left,
                bottom,
                right,
            }),
        })
        .await?;
    Ok(())
}

async fn hide() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let channel = connect().await?;
    let mut client = WindowServiceClient::new(channel);
    client.hide(Empty {}).await?;
    Ok(())
}

async fn connect() -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
    let endpoint = Endpoint::try_from(DUMMY_URI)?;
    let connect = endpoint.connect_with_connector(service_fn(|_| async {
        let started = Instant::now();
        loop {
            match ClientOptions::new().open(UI_PIPE_NAME) {
                Ok(client) => return Ok::<_, io::Error>(TokioIo::new(client)),
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                    if started.elapsed() >= PIPE_OPEN_TIMEOUT {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "candidate UI pipe remained busy",
                        ));
                    }
                    time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }));

    time::timeout(CONNECT_TIMEOUT, connect)
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out connecting to candidate UI",
            )
        })?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::windows_transport::{
        WindowsTransportCandidate, WindowsTransportCandidateWindow, WindowsTransportMarkedText,
        WindowsTransportState,
    };
    use std::collections::BTreeMap;

    fn response(kind: &str) -> WindowsTransportResponse {
        WindowsTransportResponse {
            protocol_version: 1,
            handled: true,
            input_state: WindowsTransportState {
                kind: "composing".into(),
                value: None,
            },
            input_language: None,
            effects: vec![],
            marked_text: WindowsTransportMarkedText {
                elements: vec![],
                selection_location: 0,
                selection_length: 0,
            },
            candidate_window: WindowsTransportCandidateWindow {
                kind: kind.into(),
                candidates: if kind == "hidden" {
                    vec![]
                } else {
                    vec![WindowsTransportCandidate {
                        text: "変換".into(),
                        annotation_text: Some("名詞".into()),
                        extra_values: BTreeMap::new(),
                    }]
                },
                selection_index: if kind == "hidden" { None } else { Some(0) },
            },
            prediction_candidates: vec![],
            is_empty: false,
            convert_target: "へんかん".into(),
        }
    }

    #[test]
    fn hides_when_candidate_window_is_hidden() {
        let update = CandidateWindowUpdate::from_response(&response("hidden"), None);
        assert!(!update.visible);
        assert!(update.candidates.is_empty());
    }

    #[test]
    fn publishes_candidate_text_annotation_and_selection() {
        let update = CandidateWindowUpdate::from_response(&response("composing"), Some((10, 20, 30, 40)));
        assert!(update.visible);
        assert_eq!(update.candidates[0].text, "変換");
        assert_eq!(update.candidates[0].annotation, "名詞");
        assert_eq!(update.selection_index, Some(0));
        assert_eq!(update.caret_rect, Some((10, 20, 30, 40)));
    }
}
