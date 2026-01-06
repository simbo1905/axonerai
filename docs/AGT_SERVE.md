# agt serve - Web Chat Interface

This document describes the `agt serve` command that provides a web-based chat interface for interacting with the AxonerAI agent.

## Quick Start

1. Set your API key:
```bash
export GROQ_API_KEY=your_api_key_here
# or
export ANTHROPIC_API_KEY=your_key
# or
export OPENAI_API_KEY=your_key
```

2. Start the server:
```bash
cargo run --bin agt serve
```

3. Open your browser to `http://127.0.0.1:4096`

## Command Options

```bash
agt serve [OPTIONS]

Options:
  -p, --port <PORT>  Port to listen on (default: 4096, fallback to any available) [default: 4096]
  -h, --help         Print help
```

## Features

- **WebSocket-based real-time chat**: Instant communication with the AI agent
- **Dark theme UI**: Modern, eye-friendly interface with gray-900 background and blue accents
- **Connection status indicator**: Visual feedback showing connection state
- **Message history**: Scrollable chat history with user and assistant messages
- **Keyboard shortcuts**: Press Enter to send, Shift+Enter for new line
- **No build step required**: Uses CDN-based React and Tailwind CSS

## Architecture

### HTTP Server (Axum + Tokio)
- Static file serving at `/` and `/index.html`
- Static assets served from `/assets/*`
- WebSocket endpoint at `/ws`
- CORS enabled for development

### Port Selection
The server automatically finds an available port:
1. First tries the specified/default port (4096)
2. If unavailable, falls back to any available port (OS-assigned)
3. Prints the server URL: `agt server listening on http://127.0.0.1:<port>`

### WebSocket Protocol

Messages are JSON-encoded with the following structure:

**Client → Server:**
```json
{
  "type": "user",
  "content": "your message here"
}
```

**Server → Client:**
```json
{
  "type": "assistant|system|error",
  "content": "response content"
}
```

## Development

### Running Tests
```bash
# Run CLI integration tests
cargo test --test cli_tests

# Run all tests including integration tests
cargo test --test cli_tests -- --ignored
```

### Building
```bash
# Debug build
cargo build --bin agt

# Release build
cargo build --bin agt --release
```

## Security Considerations

1. **CORS**: Currently configured for permissive development access. Restrict in production.
2. **CDN Dependencies**: Uses integrity hashes for React and React-DOM for supply chain security.
3. **API Keys**: Never commit API keys. Use environment variables.

## Troubleshooting

### Port Already in Use
If port 4096 is already in use, the server will automatically select another available port.

### WebSocket Connection Failed
- Ensure the server is running
- Check firewall settings
- Verify the URL includes `ws://` or `wss://` (handled automatically by the UI)

### API Key Not Set
The server requires one of these environment variables:
- `GROQ_API_KEY`
- `ANTHROPIC_API_KEY`
- `OPENAI_API_KEY`

Set `PROVIDER_TYPE` to choose the provider (default: "groq").
