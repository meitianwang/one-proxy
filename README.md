# tauri-cliproxy (OneProxy)

[English](README.md) | [简体中文](README.zh-CN.md)

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey.svg)](https://github.com/yourusername/tauri-cliproxy)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-blue.svg)](https://tauri.app/)

A professional desktop application that provides a unified local proxy server for multiple AI providers. Built with Tauri, it offers a beautiful management UI with OAuth authentication, quota tracking, and intelligent routing—all in a single, lightweight desktop app.

## ✨ Overview

**tauri-cliproxy** transforms your desktop into a powerful AI gateway that:

- **Unifies Multiple Providers**: Access Google Gemini, Anthropic Claude, OpenAI, and more through standardized OpenAI and Anthropic-compatible APIs
- **Simplifies Authentication**: OAuth login flows for major providers with automatic token refresh
- **Intelligent Routing**: Round-robin or fill-first strategies across multiple accounts
- **Tracks Usage**: Real-time quota monitoring and request logging
- **Integrates Seamlessly**: Works with Claude Code CLI, Continue.dev, Cursor, and any OpenAI-compatible client

Perfect for developers who want to manage multiple AI accounts, track usage across providers, or build applications that need flexible AI provider routing.

## 📑 Table of Contents

- [Features](#-features)
- [Installation](#-installation)
  - [System Requirements](#system-requirements)
  - [Download Pre-built Binaries](#download-pre-built-binaries)
  - [Build from Source](#build-from-source)
- [Quick Start](#-quick-start)
- [Account Management](#-account-management)
  - [OAuth Providers](#oauth-providers)
  - [API Key Providers](#api-key-providers)
  - [Import/Export Accounts](#importexport-accounts)
- [Configuration](#-configuration)
  - [Server Settings](#server-settings)
  - [Routing Strategies](#routing-strategies)
  - [Custom Providers](#custom-providers)
  - [Configuration File Reference](#configuration-file-reference)
- [Usage Examples](#-usage-examples)
  - [OpenAI-Compatible Clients](#openai-compatible-clients)
  - [Anthropic-Compatible Clients](#anthropic-compatible-clients)
  - [Gemini SDK](#gemini-sdk)
  - [Python Examples](#python-examples)
  - [JavaScript Examples](#javascript-examples)
- [Model Naming and Routing](#-model-naming-and-routing)
- [Claude Code Integration](#-claude-code-integration)
- [API Reference](#-api-reference)
- [Troubleshooting](#-troubleshooting)
- [FAQ](#-faq)
- [Contributing](#-contributing)
- [License](#-license)

## 🚀 Features

### Core Capabilities

- **Multi-Protocol Support**: Exposes OpenAI, Anthropic, and Gemini-compatible endpoints from a single local server
- **OAuth Authentication**: Seamless login flows for Gemini CLI (Google), Codex, Antigravity, and Kiro with automatic token refresh
- **API Key Management**: Support for Kimi and GLM providers with secure key storage
- **Intelligent Routing**: Choose between round-robin (load balancing) or fill-first (quota optimization) strategies
- **Quota Tracking**: Real-time monitoring of usage limits across all accounts with auto-refresh
- **Request Logging**: Detailed logs with filtering by protocol, model, account, and error status
- **Custom Providers**: Add your own OpenAI-compatible or Claude Code-compatible upstream services

### Advanced Features

- **Thinking Levels**: Support for reasoning/thinking parameters in Codex and Antigravity models
- **Multi-Account Routing**: Automatically switch between accounts when quotas are exceeded
- **Claude Code Integration**: One-click configuration writer for `~/.claude/settings.json`
- **System Tray Controls**: Quick access to show/hide window and start/stop server
- **LAN Access**: Optional network access for using the proxy from other devices
- **TLS Support**: HTTPS encryption for secure remote access
- **Import/Export**: Backup and restore account configurations as JSON

## 📦 Installation

### System Requirements

- **Operating System**: macOS 10.15+, Windows 10+, or Linux (Ubuntu 20.04+, Debian 11+)
- **Memory**: 512 MB RAM minimum, 1 GB recommended
- **Disk Space**: 200 MB for application and data
- **Network**: Internet connection for OAuth flows and API requests

### Download Pre-built Binaries

**Coming Soon**: Pre-built installers will be available for download.

For now, please build from source (see below).

### Build from Source

#### Prerequisites

- **Node.js**: Version 18 or higher ([Download](https://nodejs.org/))
- **Rust**: Stable toolchain ([Install via rustup](https://rustup.rs/))
- **Tauri Dependencies**: Platform-specific requirements

**macOS**:
```bash
xcode-select --install
```

**Windows**:
- Install [Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- Install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) (usually pre-installed on Windows 11)

**Linux (Debian/Ubuntu)**:
```bash
sudo apt update
sudo apt install libwebkit2gtk-4.0-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev
```

#### Build Steps

1. **Clone the repository**:
```bash
git clone https://github.com/yourusername/tauri-cliproxy.git
cd tauri-cliproxy
```

2. **Install dependencies**:
```bash
npm install
```

3. **Run in development mode**:
```bash
npm run tauri dev
```

4. **Build production binary**:
```bash
npm run tauri build
```

The built application will be in `src-tauri/target/release/bundle/`.

## 🎯 Quick Start

### First Launch

1. **Start the application**: Launch tauri-cliproxy from your applications folder or by running `npm run tauri dev`

2. **Server auto-starts**: The proxy server starts automatically on `http://127.0.0.1:8417`

3. **Add your first account**:
   - Click on the **Accounts** tab
   - Choose a provider (e.g., Gemini, Claude, Codex)
   - Click **Add Account** and follow the OAuth flow
   - Or enter an API key for supported providers

4. **Test the connection**:
```bash
curl http://127.0.0.1:8417/v1/models
```

You should see a list of available models from your configured accounts.

### Basic Usage

Once configured, use the proxy with any OpenAI-compatible client:

```bash
curl -X POST http://127.0.0.1:8417/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemini/gemini-2.0-flash-exp",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 👤 Account Management

### OAuth Providers

The following providers support OAuth authentication with automatic token refresh:

#### Google Gemini CLI

1. Click **Add Account** → **Gemini**
2. Click **Start OAuth Login**
3. Sign in with your Google account in the browser
4. Grant permissions when prompted
5. The account will appear in your accounts list

**Note**: You may need to specify a Google Cloud Project ID for API access.

#### Anthropic Claude

1. Click **Add Account** → **Claude**
2. Click **Start OAuth Login**
3. Sign in to your Anthropic account
4. Authorize the application
5. Account credentials are saved automatically

#### OpenAI Codex

1. Click **Add Account** → **Codex**
2. Click **Start OAuth Login**
3. Sign in with your OpenAI account
4. Complete the authorization flow
5. Tokens are stored securely in `~/.cli-proxy-api/`

#### Antigravity

Uses Google OAuth with specialized endpoints for extended model access.

#### Kiro (AWS CodeWhisperer)

Integrates with AWS CodeWhisperer credentials stored in `~/.aws/sso/cache/`.

### API Key Providers

For providers that use API keys instead of OAuth:

#### Kimi (Moonshot AI)

1. Click **Add Account** → **Kimi**
2. Enter your API key from [Moonshot AI Platform](https://platform.moonshot.cn/)
3. Optionally set a custom prefix for routing
4. Click **Save**

#### GLM (Zhipu AI)

1. Click **Add Account** → **GLM**
2. Enter your API key from [Zhipu AI Platform](https://open.bigmodel.cn/)
3. Configure optional settings
4. Click **Save**

### Import/Export Accounts

**Export accounts**:
1. Go to **Accounts** tab
2. Click **Export All**
3. Save the JSON file to a secure location

**Import accounts**:
1. Go to **Accounts** tab
2. Click **Import**
3. Select your previously exported JSON file
4. Accounts will be restored with their configurations

**JSON Format**:
```json
[
  {
    "provider": "gemini",
    "email": "user@example.com",
    "enabled": true,
    "token": {
      "access_token": "...",
      "refresh_token": "...",
      "expires_at": "2024-12-31T23:59:59Z"
    }
  }
]
```

## ⚙️ Configuration

### Server Settings

Access server settings from the **Dashboard** tab:

- **Host**: Bind address (default: `127.0.0.1` for localhost only)
- **Port**: Server port (default: `8417`)
- **LAN Access**: Enable to listen on `0.0.0.0` for network access
- **API Key**: Optional authentication key for proxy requests
- **TLS/HTTPS**: Enable secure connections with certificate and key files

### Routing Strategies

Control how the proxy selects accounts when multiple are available:

#### Round-Robin (Default)
Distributes requests evenly across all enabled accounts. Best for load balancing.

```yaml
routing:
  strategy: round-robin
```

#### Fill-First
Uses the first account until its quota is exhausted, then moves to the next. Best for quota optimization.

```yaml
routing:
  strategy: fill-first
```

Change the strategy in **Settings** → **Routing Strategy**.

### Custom Providers

Add your own OpenAI-compatible or Claude Code-compatible providers:

1. Go to **Settings** → **Custom Providers**
2. Click **Add Provider**
3. Configure:
   - **Name**: Provider identifier
   - **Prefix**: URL routing prefix (e.g., `custom`)
   - **Base URL**: API endpoint (e.g., `https://api.example.com`)
   - **API Keys**: One or more authentication keys
   - **Models**: List of supported model names

4. Use with the format: `custom/model-name`

**Example**:
```bash
curl -X POST http://127.0.0.1:8417/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "custom/my-model",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### Configuration File Reference

Configuration is stored in `config.yaml` in your Tauri config directory:

**macOS**: `~/Library/Application Support/com.tauri.cliproxy/config.yaml`
**Windows**: `%APPDATA%\com.tauri.cliproxy\config.yaml`
**Linux**: `~/.config/com.tauri.cliproxy/config.yaml`

**Key Settings**:

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `host` | string | `""` | Server bind address (empty = 127.0.0.1) |
| `port` | number | `8417` | Server port |
| `api_keys` | array | `[]` | API keys for proxy authentication |
| `auth_dir` | string | `~/.cli-proxy-api` | Directory for OAuth tokens |
| `routing.strategy` | string | `round-robin` | Account selection strategy |
| `quota_refresh_interval` | number | `5` | Quota refresh interval (seconds) |
| `request_retry` | number | `3` | Number of retry attempts |
| `max_retry_interval` | number | `30` | Maximum retry delay (seconds) |
| `debug` | boolean | `false` | Enable debug logging |

**Example config.yaml**:
```yaml
host: ""
port: 8417
api_keys:
  - "your-secret-key-here"
auth_dir: "~/.cli-proxy-api"
routing:
  strategy: "round-robin"
quota_refresh_interval: 5
request_retry: 3
max_retry_interval: 30
```

## 💻 Usage Examples

### OpenAI-Compatible Clients

Any tool that supports OpenAI's API can use the proxy:

```bash
curl -X POST http://127.0.0.1:8417/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{
    "model": "gemini/gemini-2.0-flash-exp",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "What is the capital of France?"}
    ],
    "temperature": 0.7,
    "max_tokens": 150
  }'
```

### Anthropic-Compatible Clients

Use Anthropic's message format:

```bash
curl -X POST http://127.0.0.1:8417/v1/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude/claude-3-5-sonnet-20241022",
    "max_tokens": 1024,
    "messages": [
      {"role": "user", "content": "Explain quantum computing in simple terms."}
    ]
  }'
```

### Gemini SDK

Use Google's native Gemini endpoints:

```bash
# List models
curl http://127.0.0.1:8417/v1beta/models

# Generate content
curl -X POST http://127.0.0.1:8417/v1beta/models/gemini-2.0-flash-exp:generateContent \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{
      "parts": [{"text": "Write a haiku about programming"}]
    }]
  }'
```

### Python Examples

**Using OpenAI SDK**:
```python
from openai import OpenAI

client = OpenAI(
    base_url="http://127.0.0.1:8417/v1",
    api_key="YOUR_API_KEY"  # Optional if not configured
)

response = client.chat.completions.create(
    model="gemini/gemini-2.0-flash-exp",
    messages=[
        {"role": "user", "content": "Hello, how are you?"}
    ]
)

print(response.choices[0].message.content)
```

**Using Anthropic SDK**:
```python
from anthropic import Anthropic

client = Anthropic(
    base_url="http://127.0.0.1:8417",
    api_key="YOUR_API_KEY"
)

message = client.messages.create(
    model="antigravity/claude-opus-4-5-thinking",
    max_tokens=1024,
    messages=[
        {"role": "user", "content": "Explain recursion with an example."}
    ]
)

print(message.content[0].text)
```

### JavaScript Examples

**Using OpenAI SDK**:
```javascript
import OpenAI from 'openai';

const client = new OpenAI({
  baseURL: 'http://127.0.0.1:8417/v1',
  apiKey: 'YOUR_API_KEY'  // Optional
});

const response = await client.chat.completions.create({
  model: 'codex/gpt-4',
  messages: [
    { role: 'user', content: 'Write a function to reverse a string' }
  ]
});

console.log(response.choices[0].message.content);
```

**Using fetch API**:
```javascript
const response = await fetch('http://127.0.0.1:8417/v1/chat/completions', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'Authorization': 'Bearer YOUR_API_KEY'
  },
  body: JSON.stringify({
    model: 'gemini/gemini-2.0-flash-exp',
    messages: [
      { role: 'user', content: 'Hello!' }
    ]
  })
});

const data = await response.json();
console.log(data.choices[0].message.content);
```

## 🔀 Model Naming and Routing

Model names determine which provider handles the request.

### Format

- **Standard**: `provider/model-name`
- **Alternative**: `provider:model-name`

### Built-in Providers

| Prefix | Provider | Example |
|--------|----------|---------|
| `gemini` | Google Gemini | `gemini/gemini-2.0-flash-exp` |
| `codex` | OpenAI Codex | `codex/gpt-4` |
| `openai` | OpenAI (alias for codex) | `openai/gpt-4-turbo` |
| `claude` | Anthropic Claude | `claude/claude-3-5-sonnet-20241022` |
| `antigravity` | Antigravity (Google-based) | `antigravity/claude-opus-4-5-thinking` |
| `kiro` | AWS CodeWhisperer | `kiro/model-name` |
| `kimi` | Moonshot AI | `kimi/moonshot-v1-8k` |
| `glm` | Zhipu AI | `glm/glm-4` |

### Thinking Levels

Some providers support reasoning/thinking parameters in the model name:

**Codex**:
- Format: `codex/<level>/<model>`
- Levels: `low`, `medium`, `high`, `xhigh`
- Example: `codex/high/gpt-4`

**Antigravity** (Gemini 3 only):
- Format: `antigravity/<level>/<model>`
- Levels: `minimal`, `low`, `medium`, `high`
- Example: `antigravity/medium/gemini-3-flash`

**Note**: Not all models support thinking levels. Use `GET /v1/models` to see available variants. Unsupported levels return HTTP 400.

### Custom Provider Routing

For custom providers configured in Settings:

```bash
# If you configured a provider with prefix "myapi"
curl -X POST http://127.0.0.1:8417/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "myapi/custom-model",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

## 🔧 Claude Code Integration

Automatically configure Claude Code CLI to use this proxy:

1. Go to **Dashboard** tab
2. Find the **Claude Code Integration** section
3. Click **Update Claude Code Settings**
4. The app will write to `~/.claude/settings.json`:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8417",
    "ANTHROPIC_AUTH_TOKEN": "your-api-key",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-20250514",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-3-5-sonnet-20241022",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-3-5-haiku-20241022"
  }
}
```

5. Restart Claude Code CLI to apply changes

Now Claude Code will route all requests through your proxy, allowing you to:
- Use multiple Claude accounts with automatic rotation
- Track Claude Code usage in the request logs
- Switch between different Claude providers (official, Antigravity, etc.)

## 📚 API Reference

### Endpoints

#### OpenAI-Compatible

- `GET /v1/models` - List available models
- `POST /v1/chat/completions` - Chat completions (streaming supported)
- `POST /v1/completions` - Text completions

#### Anthropic-Compatible

- `POST /v1/messages` - Create message (streaming supported)
- `POST /v1/messages/count_tokens` - Count tokens in messages

#### Gemini-Compatible

- `GET /v1beta/models` - List Gemini models
- `POST /v1beta/models/{model}:generateContent` - Generate content
- `POST /v1beta/models/{model}:streamGenerateContent` - Stream content

### Authentication

If API keys are configured, include in requests:

```
Authorization: Bearer YOUR_API_KEY
```

Or pass the key directly without "Bearer" prefix.

### Error Codes

| Code | Meaning | Solution |
|------|---------|----------|
| 400 | Bad Request | Check model name and request format |
| 401 | Unauthorized | Verify API key or disable authentication |
| 404 | Not Found | Check endpoint URL |
| 429 | Rate Limited | Proxy will auto-retry with another account |
| 500 | Server Error | Check logs for details |
| 502 | Bad Gateway | Upstream provider error |

### Response Format

Follows OpenAI and Anthropic specifications. See their official documentation for details.

## 🔍 Troubleshooting

### 401 Unauthorized Errors

**Problem**: Requests return 401 even with valid credentials.

**Solutions**:
- Check if API keys are configured in Settings
- Verify the `Authorization` header format: `Bearer YOUR_KEY`
- Try disabling API key authentication in Dashboard
- Ensure the key matches one in `config.yaml`

### No Models Listed

**Problem**: `GET /v1/models` returns empty array.

**Solutions**:
- Add at least one account in the Accounts tab
- Ensure accounts are enabled (toggle switch)
- Click **Refresh Quota** to update account status
- Check account credentials haven't expired

### Port Already in Use

**Problem**: Server fails to start with "address already in use" error.

**Solutions**:
- Change the port in Dashboard → Server Settings
- Find and stop the conflicting process:
  ```bash
  # macOS/Linux
  lsof -i :8417
  kill -9 <PID>

  # Windows
  netstat -ano | findstr :8417
  taskkill /PID <PID> /F
  ```

### OAuth Login Fails

**Problem**: Browser opens but authorization doesn't complete.

**Solutions**:
- Ensure you're signed in to the correct account
- Check firewall isn't blocking localhost callbacks
- Try manual OAuth if automatic fails
- Clear browser cookies for the provider
- Check system time is correct (OAuth tokens are time-sensitive)

### Quota Exceeded Errors

**Problem**: Requests fail with quota exceeded messages.

**Solutions**:
- Add additional accounts for the same provider
- Enable round-robin routing to distribute load
- Check quota limits in the Dashboard
- Wait for quota reset (usually daily or monthly)
- Configure `quota_exceeded.switch_project` in config

### Connection Timeouts

**Problem**: Requests hang or timeout.

**Solutions**:
- Check internet connection
- Verify upstream provider status
- Increase `max_retry_interval` in config
- Check proxy settings if behind corporate firewall
- Enable debug logging to see detailed errors

### Debug Mode

Enable detailed logging:

1. Edit `config.yaml` and set `debug: true`
2. Restart the application
3. Check logs in:
   - **macOS**: `~/Library/Logs/com.tauri.cliproxy/`
   - **Windows**: `%APPDATA%\com.tauri.cliproxy\logs\`
   - **Linux**: `~/.local/share/com.tauri.cliproxy/logs/`

Or run from terminal:
```bash
npm run tauri dev
```

## ❓ FAQ

### Can I use this with multiple Claude Code instances?

Yes, but they'll share the same configuration. Each instance will use the proxy independently.

### Does this work with Continue.dev or Cursor?

Yes! Configure them to use OpenAI-compatible API with:
- Base URL: `http://127.0.0.1:8417/v1`
- API Key: Your configured proxy key (or leave empty if none set)

### How do I use multiple Google accounts?

Add each account separately through the OAuth flow. The proxy will rotate between them based on your routing strategy.

### Is my data secure?

- OAuth tokens are stored locally in `~/.cli-proxy-api/`
- API keys are stored in `config.yaml` (consider file permissions)
- All requests stay on your machine unless you enable LAN access
- Enable TLS for encrypted remote access

### Can I run this on a server?

Yes! Enable LAN access and configure TLS for secure remote access. Consider using a reverse proxy (nginx, Caddy) for production deployments.

### What's the difference between round-robin and fill-first?

- **Round-robin**: Distributes requests evenly across accounts (better for load balancing)
- **Fill-first**: Uses one account until quota exhausted (better for quota optimization)

### How do I backup my configuration?

1. Export accounts from the Accounts tab
2. Copy `config.yaml` from your Tauri config directory
3. Store both files securely

### Does this support streaming responses?

Yes! Both OpenAI and Anthropic streaming formats are supported. Use `stream: true` in your requests.

### Can I use this commercially?

Check the license file. Generally, the proxy itself is open source, but you must comply with each AI provider's terms of service.

## 🤝 Contributing

Contributions are welcome! Here's how you can help:

### Reporting Issues

Found a bug? Please open an issue with:
- Clear description of the problem
- Steps to reproduce
- Expected vs actual behavior
- System information (OS, version)
- Relevant logs (with sensitive data removed)

### Feature Requests

Have an idea? Open an issue with:
- Use case description
- Proposed solution
- Alternative approaches considered

### Development

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/amazing-feature`
3. Make your changes
4. Test thoroughly
5. Commit: `git commit -m 'Add amazing feature'`
6. Push: `git push origin feature/amazing-feature`
7. Open a Pull Request

### Code Style

- **Rust**: Follow `rustfmt` defaults
- **TypeScript**: Follow project ESLint config
- **Commits**: Use conventional commits format

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built with [Tauri](https://tauri.app/)
- Inspired by [CLI Proxy API](https://github.com/example/cli-proxy-api)
- Thanks to all contributors and users

---

**Need Help?** Open an issue on [GitHub](https://github.com/yourusername/tauri-cliproxy/issues)

**Star this project** if you find it useful! ⭐
