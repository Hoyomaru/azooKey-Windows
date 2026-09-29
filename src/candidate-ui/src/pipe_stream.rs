use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use async_stream::stream;
use futures_core::stream::Stream;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions},
};
use tonic::transport::server::Connected;
use windows::{
    core::w,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
    },
};

fn create_pipe(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut descriptor = PSECURITY_DESCRIPTOR(std::ptr::null_mut());
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:(A;;GA;;;AC)(A;;GA;;;RC)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;BU)S:(ML;;NW;;;LW)"),
            1,
            &mut descriptor,
            None,
        )
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    }

    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };

    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .pipe_mode(PipeMode::Byte)
        .max_instances(4)
        .in_buffer_size(4096)
        .out_buffer_size(4096)
        .reject_remote_clients(true);

    let result = unsafe {
        options.create_with_security_attributes_raw(
            format!(r"\\.\pipe\{name}"),
            &mut attributes as *mut SECURITY_ATTRIBUTES as *mut _,
        )
    };

    unsafe {
        let _ = LocalFree(HLOCAL(descriptor.0));
    }

    result
}

pub struct PipeConnection {
    inner: NamedPipeServer,
}

impl Connected for PipeConnection {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

impl AsyncRead for PipeConnection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}

impl AsyncWrite for PipeConnection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn create_next(name: &str) -> std::io::Result<NamedPipeServer> {
    loop {
        match create_pipe(name, false) {
            Ok(server) => return Ok(server),
            Err(error) => {
                tracing::warn!("candidate UI pipe create failed: {error:?}");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

pub fn stream(name: &str) -> impl Stream<Item = std::io::Result<PipeConnection>> {
    let name = name.to_string();
    stream! {
        let mut server = create_pipe(&name, true)?;
        loop {
            server.connect().await?;
            let next = create_next(&name).await;
            yield Ok(PipeConnection { inner: server });
            server = next?;
        }
    }
}
