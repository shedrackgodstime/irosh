# Transfer & Control Protocols

`irosh` ensures the robustness and visual integrity of the interactive SSH terminal by routing all non-interactive data (like file transfers and peer metadata) over separate, dedicated P2P streams.

This document describes the custom framing used on these side-channels.

## Frame Architecture

Because the underlying Iroh streams are raw TCP-like byte pipes (`AsyncRead` / `AsyncWrite`), `irosh` implements a lightweight framing codec. 

Every frame transmitted over a side-stream consists of:
1. **Magic Bytes (4 bytes)**: A static marker ensuring stream sync.
2. **Protocol Version (1 byte)**: Currently `0x01`. 
3. **Payload Type (1 byte)**: An enum representing the specific command or data.
4. **Length Prefix (4 bytes)**: A `u32` (Big Endian) representing the exact size of the payload.
5. **Payload (Variable)**: A JSON-encoded data structure or raw binary bytes.

## 1. The Metadata Stream

Immediately upon connecting, the client opens a unidirectional Metadata Stream to the server. This stream acts as a "handshake" auxiliary channel.

**Current Frames**:
- `MetadataRequest`: Client requests node information.
- `MetadataResponse`: Server replies with an optional JSON payload containing its `hostname`, `os`, and active `username`. 

The client uses this advisory data to automatically suggest user-friendly alias names (e.g., `linux-box-root`). This data is *never* used for security decisions.

## 2. File Transfer Streams

When a user initiates a local `:put` or `:get` command, the client opens a new bidirectional Transfer Stream.

File transfers use a chunked protocol to handle massive files without memory exhaustion.

### Upload Protocol (`:put`)
1. **Client -> Server**: `PutRequest` (JSON payload with remote destination path and file size).
2. **Server -> Client**: `PutReady` (Aanck to proceed, or an `Error` frame if the target exists/is denied).
3. **Client -> Server**: Continuous stream of `Chunk` frames (Raw binary).
4. **Client -> Server**: `TransferComplete` (Signaling EOF).

### Download Protocol (`:get`)
1. **Client -> Server**: `GetRequest` (JSON payload with the requested remote path).
2. **Server -> Client**: `GetReady` (Contains the probed file size, or an `Error` frame if missing).
3. **Server -> Client**: Continuous stream of `Chunk` frames (Raw binary).
4. **Server -> Client**: `TransferComplete` (Signaling EOF).

### Concurrency and Integrity
By isolating file chunks to ephemeral side-streams, the library ensures that an interrupted `CTRL-C` transfer does not crash the interactive SSH shell or leave binary garbage on the screen. Transfer streams automatically reap themselves upon TCP disconnect or EOF.
