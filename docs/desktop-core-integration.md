# Desktop-core integration

This branch connects the Windows TSF rewrite to the azooKey desktop conversion core without
placing Swift or Zenzai inside application processes.

Architecture:

```text
Windows app
  -> Rust TSF DLL
  -> Named Pipe / gRPC
  -> conversion-server.exe
  -> dedicated EngineWorker thread
  -> Swift azooKey Desktop engine
```

Current state keeps the existing echo behavior behind `ConversionEngine` while introducing
a serialized engine owner and a versioned `Handle` RPC. The Swift-backed engine will replace
`EchoEngine` without changing TSF or IPC ownership.


## Swift bridge

The conversion server loads `AzooKeyDesktopEngine.dll` at runtime. The bridge exposes a
small versioned C ABI for engine creation, request handling, response-buffer release and
engine destruction. The Rust server owns the DLL through a dedicated `EngineWorker`
thread and falls back safely when the DLL cannot be loaded or its ABI version is unsupported.

CI builds the Desktop fork on Windows and exercises a real Rust -> DLL -> Swift -> Rust
roundtrip so the ABI boundary is tested independently from TSF.
