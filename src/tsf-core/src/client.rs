use std::{
    error::Error,
    io,
    sync::OnceLock,
    time::{Duration, Instant},
};

use hyper_util::rt::TokioIo;
use shared::{
    conversion::{
        conversion_service_client::ConversionServiceClient, ConvertRequest, EngineRequest,
    },
    windows_transport::{
        WindowsTransportRequest, WindowsTransportResponse, WINDOWS_TRANSPORT_PROTOCOL_VERSION,
    },
};
use tokio::{
    net::windows::named_pipe::ClientOptions,
    runtime::Runtime,
    time,
};
use tonic::transport::Endpoint;
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

const PIPE_NAME: &str = r"\\.\pipe\azookey-conversion";
const DUMMY_URI: &str = "http://[::]:50051";
const ENGINE_RPC_PROTOCOL_VERSION: u32 = 1;

const PIPE_OPEN_TIMEOUT: Duration = Duration::from_millis(150);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

type ClientError = Box<dyn Error + Send + Sync>;

fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}

/// Temporary compatibility endpoint used by the early rewrite prototype.
pub fn convert(text: &str) -> String {
    let text = text.to_string();
    get_runtime().block_on(async move {
        match do_convert(&text).await {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    "Conversion server unavailable: {:?}, using fallback",
                    error
                );
                text
            }
        }
    })
}

/// Sends one stable Windows transport request to the shared desktop engine.
///
/// This call is synchronous because TSF currently calls it from the key path, but all
/// waits are strictly bounded. The next TSF integration step moves the request outside
/// the write EditSession so application COM state is never held while IPC is pending.
pub fn handle(request: &WindowsTransportRequest) -> Result<WindowsTransportResponse, ClientError> {
    let payload = serde_json::to_vec(request)?;
    get_runtime().block_on(async move {
        let response = do_handle(payload).await?;
        Ok(serde_json::from_slice(&response)?)
    })
}

async fn connect_client() -> Result<ConversionServiceClient<tonic::transport::Channel>, ClientError> {
    let channel = time::timeout(
        CONNECT_TIMEOUT,
        Endpoint::try_from(DUMMY_URI)?.connect_with_connector(service_fn(|_| async {
            let started = Instant::now();
            loop {
                match ClientOptions::new().open(PIPE_NAME) {
                    Ok(client) => return Ok::<_, io::Error>(TokioIo::new(client)),
                    Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                        if started.elapsed() >= PIPE_OPEN_TIMEOUT {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "azooKey conversion pipe remained busy",
                            ));
                        }
                        time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        })),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "conversion server connect timed out"))??;

    Ok(ConversionServiceClient::new(channel))
}

async fn do_handle(payload: Vec<u8>) -> Result<Vec<u8>, ClientError> {
    let mut client = connect_client().await?;
    let response = time::timeout(
        REQUEST_TIMEOUT,
        client.handle(EngineRequest {
            protocol_version: ENGINE_RPC_PROTOCOL_VERSION,
            payload,
        }),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "conversion engine request timed out"))??
    .into_inner();

    if response.protocol_version != ENGINE_RPC_PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported engine RPC protocol version: {}",
                response.protocol_version
            ),
        )
        .into());
    }

    Ok(response.payload)
}

async fn do_convert(text: &str) -> Result<String, ClientError> {
    let mut client = connect_client().await?;
    let response = time::timeout(
        REQUEST_TIMEOUT,
        client.convert(ConvertRequest {
            text: text.to_string(),
        }),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "compat conversion request timed out"))??
    .into_inner();

    Ok(response.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_budget_is_bounded_for_key_path() {
        assert!(PIPE_OPEN_TIMEOUT < REQUEST_TIMEOUT);
        assert!(CONNECT_TIMEOUT < REQUEST_TIMEOUT);
        assert!(REQUEST_TIMEOUT <= Duration::from_millis(500));
    }

    #[test]
    fn transport_protocol_matches_shared_contract() {
        assert_eq!(WINDOWS_TRANSPORT_PROTOCOL_VERSION, 1);
        assert_eq!(ENGINE_RPC_PROTOCOL_VERSION, 1);
    }
}
