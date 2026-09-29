mod engine_worker;
mod pipe_stream;
mod swift_engine;

use engine_worker::{EchoEngine, EngineWorker};
use swift_engine::SwiftEngine;
use shared::conversion::conversion_service_server::{ConversionService, ConversionServiceServer};
use shared::conversion::{ConvertRequest, ConvertResponse, EngineRequest, EngineResponse};
use tonic::{transport::Server, Request, Response, Status};

#[derive(Clone)]
struct ConversionServiceImpl {
    engine: EngineWorker,
}

impl ConversionServiceImpl {
    const ENGINE_PROTOCOL_VERSION: u32 = 1;

    fn new(engine: EngineWorker) -> Self {
        Self { engine }
    }

    async fn run_engine(&self, payload: String) -> Result<String, Status> {
        let engine = self.engine.clone();
        tokio::task::spawn_blocking(move || engine.handle(payload))
            .await
            .map_err(|error| Status::internal(format!("engine worker join failed: {error}")))?
            .map_err(|error| Status::unavailable(error.to_string()))
    }
}

#[tonic::async_trait]
impl ConversionService for ConversionServiceImpl {
    async fn convert(
        &self,
        request: Request<ConvertRequest>,
    ) -> Result<Response<ConvertResponse>, Status> {
        let text = request.into_inner().text;
        let converted = self.run_engine(text).await?;
        Ok(Response::new(ConvertResponse { text: converted }))
    }

    async fn handle(
        &self,
        request: Request<EngineRequest>,
    ) -> Result<Response<EngineResponse>, Status> {
        let request = request.into_inner();
        if request.protocol_version != Self::ENGINE_PROTOCOL_VERSION {
            return Err(Status::failed_precondition(format!(
                "unsupported engine protocol version: {}",
                request.protocol_version
            )));
        }

        let payload = String::from_utf8(request.payload)
            .map_err(|_| Status::invalid_argument("engine payload must be UTF-8 JSON"))?;
        let payload = self.run_engine(payload).await?;

        Ok(Response::new(EngineResponse {
            protocol_version: Self::ENGINE_PROTOCOL_VERSION,
            payload: payload.into_bytes(),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let pipe_name = "azookey-conversion";
    tracing::info!("Starting conversion server on \\\\.\\pipe\\{}", pipe_name);

    let engine = match SwiftEngine::load_default() {
        Ok(engine) => {
            tracing::info!("Loaded AzooKey Desktop Swift engine");
            EngineWorker::spawn(engine)
        }
        Err(error) => {
            tracing::warn!(
                "Swift desktop engine unavailable: {}; falling back to echo engine",
                error
            );
            EngineWorker::spawn(EchoEngine)
        }
    };
    let stream = pipe_stream::create_pipe_stream(pipe_name);
    let svc = ConversionServiceServer::new(ConversionServiceImpl::new(engine));

    Server::builder()
        .add_service(svc)
        .serve_with_incoming(stream)
        .await?;

    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use hyper_util::rt::TokioIo;
    use shared::conversion::conversion_service_client::ConversionServiceClient;
    use shared::conversion::{ConvertRequest, EngineRequest};
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::time::Duration;
    use tonic::transport::Endpoint;
    use tower::service_fn;

    #[tokio::test]
    async fn test_versioned_engine_roundtrip() {
        let pipe_name = format!("azookey-test-engine-{}", std::process::id());

        let stream = pipe_stream::create_pipe_stream(&pipe_name);
        let engine = EngineWorker::spawn(EchoEngine);
        let server = ConversionServiceServer::new(ConversionServiceImpl::new(engine));
        let server_handle = tokio::spawn(async move {
            Server::builder()
                .add_service(server)
                .serve_with_incoming(stream)
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let pipe_path = format!(r"\\.\pipe\{}", pipe_name);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(service_fn(move |_| {
                let pipe_path = pipe_path.clone();
                async move {
                    let client = ClientOptions::new().open(&pipe_path)?;
                    Ok::<_, std::io::Error>(TokioIo::new(client))
                }
            }))
            .await
            .unwrap();

        let mut client = ConversionServiceClient::new(channel);
        let payload = br#"{"type":"ping"}"#.to_vec();
        let response = client
            .handle(EngineRequest {
                protocol_version: ConversionServiceImpl::ENGINE_PROTOCOL_VERSION,
                payload: payload.clone(),
            })
            .await
            .unwrap()
            .into_inner();

        assert_eq!(
            response.protocol_version,
            ConversionServiceImpl::ENGINE_PROTOCOL_VERSION
        );
        assert_eq!(response.payload, payload);

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_grpc_roundtrip() {
        let pipe_name = format!("azookey-test-grpc-{}", std::process::id());

        let stream = pipe_stream::create_pipe_stream(&pipe_name);
        let engine = EngineWorker::spawn(EchoEngine);
        let server = ConversionServiceServer::new(ConversionServiceImpl::new(engine));
        let server_handle = tokio::spawn(async move {
            Server::builder()
                .add_service(server)
                .serve_with_incoming(stream)
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let pipe_path = format!(r"\\.\pipe\{}", pipe_name);
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(service_fn(move |_| {
                let pipe_path = pipe_path.clone();
                async move {
                    let client = ClientOptions::new().open(&pipe_path)?;
                    Ok::<_, std::io::Error>(TokioIo::new(client))
                }
            }))
            .await
            .unwrap();

        let mut client = ConversionServiceClient::new(channel);
        let response = client
            .convert(ConvertRequest {
                text: "hello".into(),
            })
            .await
            .unwrap();
        assert_eq!(response.into_inner().text, "hello");

        server_handle.abort();
    }
}
