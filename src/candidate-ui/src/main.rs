use anyhow::Result;
use shared::window::{
    window_service_server::{WindowService, WindowServiceServer},
    Empty, SetCandidatesRequest, SetPositionRequest, SetSelectionRequest,
};
use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::windows::WindowExtWindows,
};
use tokio::sync::mpsc;
use tonic::{transport::Server, Request, Response, Status};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE},
};

mod candidate;
mod pipe_stream;
mod utils;

#[derive(Debug, Clone)]
struct UiCandidate {
    text: String,
    annotation: String,
}

#[derive(Debug, Clone)]
enum WindowAction {
    Show,
    Hide,
    SetPosition {
        top: i32,
        left: i32,
        bottom: i32,
        right: i32,
    },
    SetCandidates(Vec<UiCandidate>),
    SetSelection(i32),
}

#[derive(Clone)]
struct WindowController {
    sender: mpsc::Sender<WindowAction>,
}

#[tonic::async_trait]
impl WindowService for WindowController {
    async fn show(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
        self.sender
            .send(WindowAction::Show)
            .await
            .map_err(|_| Status::unavailable("candidate UI event loop stopped"))?;
        Ok(Response::new(Empty {}))
    }

    async fn hide(&self, _request: Request<Empty>) -> Result<Response<Empty>, Status> {
        self.sender
            .send(WindowAction::Hide)
            .await
            .map_err(|_| Status::unavailable("candidate UI event loop stopped"))?;
        Ok(Response::new(Empty {}))
    }

    async fn set_position(
        &self,
        request: Request<SetPositionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let position = request
            .into_inner()
            .position
            .ok_or_else(|| Status::invalid_argument("position is required"))?;
        self.sender
            .send(WindowAction::SetPosition {
                top: position.top,
                left: position.left,
                bottom: position.bottom,
                right: position.right,
            })
            .await
            .map_err(|_| Status::unavailable("candidate UI event loop stopped"))?;
        Ok(Response::new(Empty {}))
    }

    async fn set_candidates(
        &self,
        request: Request<SetCandidatesRequest>,
    ) -> Result<Response<Empty>, Status> {
        let candidates = request
            .into_inner()
            .candidates
            .into_iter()
            .map(|item| UiCandidate {
                text: item.text,
                annotation: item.annotation,
            })
            .collect();
        self.sender
            .send(WindowAction::SetCandidates(candidates))
            .await
            .map_err(|_| Status::unavailable("candidate UI event loop stopped"))?;
        Ok(Response::new(Empty {}))
    }

    async fn set_selection(
        &self,
        request: Request<SetSelectionRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.sender
            .send(WindowAction::SetSelection(request.into_inner().index))
            .await
            .map_err(|_| Status::unavailable("candidate UI event loop stopped"))?;
        Ok(Response::new(Empty {}))
    }
}

#[derive(Debug)]
enum UserEvent {
    Window(WindowAction),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event()
        .with_any_thread(true)
        .build();
    let proxy = event_loop.create_proxy();

    let window = candidate::create_candidate_window(&event_loop)?;
    window.set_inner_size(LogicalSize::new(340.0, 250.0));

    let webview = candidate::create_candidate_webview()?.build(&window)?;

    let (sender, mut receiver) = mpsc::channel::<WindowAction>(32);
    let controller = WindowController { sender };

    tokio::spawn(async move {
        let incoming = pipe_stream::stream("azookey-ui");
        if let Err(error) = Server::builder()
            .add_service(WindowServiceServer::new(controller))
            .serve_with_incoming(incoming)
            .await
        {
            tracing::error!("candidate UI server stopped: {error:?}");
        }
    });

    let event_proxy = proxy.clone();
    tokio::spawn(async move {
        while let Some(action) = receiver.recv().await {
            if event_proxy.send_event(UserEvent::Window(action)).is_err() {
                break;
            }
        }
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Window(action)) => match action {
                WindowAction::Show => unsafe {
                    let _ = ShowWindow(
                        HWND(window.hwnd() as *mut std::ffi::c_void),
                        SW_SHOWNOACTIVATE,
                    );
                },
                WindowAction::Hide => unsafe {
                    let _ = ShowWindow(HWND(window.hwnd() as *mut std::ffi::c_void), SW_HIDE);
                },
                WindowAction::SetPosition {
                    top,
                    left,
                    bottom,
                    right,
                } => {
                    let (x, y) = utils::candidate_position(top, left, bottom, right, &window);
                    window.set_outer_position(PhysicalPosition::new(x, y));
                }
                WindowAction::SetCandidates(candidates) => {
                    let values = candidates
                        .into_iter()
                        .map(|candidate| {
                            serde_json::json!({
                                "text": candidate.text,
                                "annotation": candidate.annotation,
                            })
                        })
                        .collect::<Vec<_>>();
                    if let Ok(json) = serde_json::to_string(&values) {
                        let _ = webview.evaluate_script(&format!("updateCandidates({json})"));
                    }
                }
                WindowAction::SetSelection(index) => {
                    let _ = webview.evaluate_script(&format!("updateSelection({index})"));
                }
            },
            _ => {}
        }
    });
}
