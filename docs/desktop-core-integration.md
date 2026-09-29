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
