use std::sync::OnceLock;
use std::time::Duration;

use tokio::net::windows::named_pipe::ClientOptions;
use tokio::runtime::Runtime;
use tokio::time;
use hyper_util::rt::TokioIo;
use tonic::transport::Endpoint;
use tower::service_fn;
use windows::Win32::Foundation::ERROR_PIPE_BUSY;

use shared::conversion::conversion_service_client::ConversionServiceClient;
use shared::conversion::{ConvertRequest, EngineRequest};

const PIPE_NAME: &str = r"\\.\pipe\azookey-conversion";
const DUMMY_URI: &str = "http://[::]:50051";
const ENGINE_PROTOCOL_VERSION: u32 = 1;
const PIPE_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const PIPE_BUSY_RETRY_DELAY: Duration = Duration::from_millis(10);
const RPC_TIMEOUT: Duration = Duration::from_millis(500);

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}

pub fn convert(text: &str) -> String {
    let text = text.to_string();
    get_runtime().block_on(async move {
        match do_convert(&text).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("Conversion server unavailable: {:?}, using fallback", e);
                text
            }
        }
    })
}


pub fn handle_engine_json(payload: &str) -> Result<String, String> {
    let payload = payload.to_string();
    get_runtime()
        .block_on(async move {
            let response = do_handle_engine(payload.into_bytes()).await?;
            String::from_utf8(response)
                .map_err(|error| format!("engine response was not UTF-8: {error}"))
        })
        .map_err(|error: Box<dyn std::error::Error + Send + Sync>| error.to_string())
}

async fn connect_channel(
) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
    let connect = Endpoint::try_from(DUMMY_URI)?
        .connect_with_connector(service_fn(|_| async {
            let started = time::Instant::now();
            loop {
                match ClientOptions::new().open(PIPE_NAME) {
                    Ok(client) => {
                        return Ok::<_, std::io::Error>(TokioIo::new(client));
                    }
                    Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                        if started.elapsed() >= PIPE_CONNECT_TIMEOUT {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "conversion pipe remained busy",
                            ));
                        }
                        time::sleep(PIPE_BUSY_RETRY_DELAY).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        }));

    time::timeout(PIPE_CONNECT_TIMEOUT, connect)
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out connecting to conversion server",
            )
        })?
        .map_err(Into::into)
}

async fn do_handle_engine(
    payload: Vec<u8>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let channel = connect_channel().await?;
    let mut client = ConversionServiceClient::new(channel);

    let request = EngineRequest {
        protocol_version: ENGINE_PROTOCOL_VERSION,
        payload,
    };

    let response = time::timeout(RPC_TIMEOUT, client.handle(request))
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "conversion engine request timed out",
            )
        })??;

    let response = response.into_inner();
    if response.protocol_version != ENGINE_PROTOCOL_VERSION {
        return Err(format!(
            "unexpected engine protocol version: {}",
            response.protocol_version
        )
        .into());
    }

    Ok(response.payload)
}

async fn do_convert(
    text: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let channel = connect_channel().await?;

    let mut client = ConversionServiceClient::new(channel);
    let response = client
        .convert(ConvertRequest {
            text: text.to_string(),
        })
        .await?;
    Ok(response.into_inner().text)
}
