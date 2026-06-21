use std::io;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum WeatherError {
    #[error("{0}")]
    Network(#[from] NetworkError),

    #[error("{0}")]
    Config(#[from] ConfigError),

    #[error("{0}")]
    Terminal(#[from] TerminalError),

    #[error("{0}")]
    Geolocation(#[from] GeolocationError),

    #[error("{0}")]
    Data(#[from] DataError),
}

#[derive(ThisError, Debug)]
pub enum DataError {
    #[error("Provider returned no data")]
    NoData,

    #[error("Failed to parse data: {0}")]
    SerdeParseError(#[source] serde_json::Error),

    #[error("Failed to parse data: {0}")]
    ChronoParseError(#[source] chrono::ParseError),

    #[error("Provider returned bad data: {0}")]
    BadData(String),
}

#[derive(ThisError, Debug)]
pub enum NetworkError {
    #[error("failed to create HTTP client: {0}")]
    ClientCreation(#[source] reqwest::Error),

    #[error("DNS resolution failed for {url}")]
    DnsFailure {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("connection timeout after {timeout_secs}s for {url}")]
    Timeout { url: String, timeout_secs: u64 },

    #[error("connection refused for {url}")]
    ConnectionRefused { url: String },

    #[error("HTTP request failed for {url}: {status}")]
    HttpError {
        url: String,
        status: u16,
        #[source]
        source: reqwest::Error,
    },

    #[error("failed to parse JSON response from {url}")]
    JsonParse {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("network error: {0}")]
    Other(#[from] reqwest::Error),
}

impl NetworkError {
    pub fn from_reqwest(error: reqwest::Error, url: &str, timeout_secs: u64) -> Self {
        if error.is_timeout() {
            NetworkError::Timeout {
                url: url.to_string(),
                timeout_secs,
            }
        } else if error.is_connect() {
            let error_msg = error.to_string();
            if error_msg.contains("dns") || error_msg.contains("DNS") {
                return NetworkError::DnsFailure {
                    url: url.to_string(),
                    source: error,
                };
            }
            if error_msg.contains("Connection refused") || error_msg.contains("refused") {
                return NetworkError::ConnectionRefused {
                    url: url.to_string(),
                };
            }
            NetworkError::Other(error)
        } else if error.is_status() {
            if let Some(status) = error.status() {
                return NetworkError::HttpError {
                    url: url.to_string(),
                    status: status.as_u16(),
                    source: error,
                };
            }
            NetworkError::Other(error)
        } else if error.is_decode() {
            NetworkError::JsonParse {
                url: url.to_string(),
                source: error,
            }
        } else {
            NetworkError::Other(error)
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            NetworkError::Timeout { .. }
                | NetworkError::ConnectionRefused { .. }
                | NetworkError::DnsFailure { .. }
        )
    }

    pub fn user_friendly_message(&self) -> String {
        match self {
            NetworkError::DnsFailure { url, .. } => {
                format!("无法访问 {url}。请检查网络连接或 DNS 设置。")
            }
            NetworkError::Timeout { url, timeout_secs } => {
                format!(
                    "请求 {url} 超时 ({timeout_secs}秒)。请检查网络连接。"
                )
            }
            NetworkError::ConnectionRefused { url } => {
                format!("无法连接 {url}。服务可能已停止。")
            }
            NetworkError::HttpError { url, status, .. } => {
                format!("来自 {url} 的服务器错误: HTTP {status}")
            }
            NetworkError::JsonParse { url, .. } => {
                format!("从 {url} 接收到无效数据")
            }
            NetworkError::ClientCreation(_) => "HTTP 客户端初始化失败".to_string(),
            NetworkError::Other(e) => format!("网络错误: {e}"),
        }
    }
}

#[derive(ThisError, Debug)]
pub enum ConfigError {
    #[error("failed to read config file at {path}")]
    ReadError {
        path: String,
        #[source]
        source: io::Error,
    },

    #[error("invalid TOML syntax in config file")]
    ParseError(#[from] toml::de::Error),

    #[error("could not determine config directory (check $XDG_CONFIG_HOME or $HOME)")]
    NoConfigDir,

    #[error("invalid latitude {0} (must be between -90 and 90)")]
    InvalidLatitude(f64),

    #[error("invalid longitude {0} (must be between -180 and 180)")]
    InvalidLongitude(f64),

    #[error("invalid value for ${name} (expected a float, got {value:?})")]
    InvalidEnvVar { name: &'static str, value: String },
    #[error("invalid API Key ({0})")]
    InvalidAPIKey(String),
}

impl ConfigError {
    #[allow(dead_code)]
    pub fn kind(&self) -> &str {
        match self {
            ConfigError::ReadError { .. } => "ReadError",
            ConfigError::ParseError(_) => "ParseError",
            ConfigError::NoConfigDir => "NoConfigDir",
            ConfigError::InvalidLatitude(_) => "InvalidLatitude",
            ConfigError::InvalidLongitude(_) => "InvalidLongitude",
            ConfigError::InvalidEnvVar { .. } => "InvalidEnvVar",
            ConfigError::InvalidAPIKey(_) => "InvalidAPIKey",
        }
    }
}

#[derive(ThisError, Debug)]
pub enum TerminalError {
    #[error("terminal is too small (minimum: {min_width}x{min_height}, current: {width}x{height})")]
    TooSmall {
        width: u16,
        height: u16,
        min_width: u16,
        min_height: u16,
    },

    #[error("not running in a terminal (output is redirected or piped)")]
    NotATty,

    #[error("failed to enable raw mode")]
    RawModeError(#[source] io::Error),

    #[error("failed to get terminal size")]
    SizeError(#[source] io::Error),

    #[error("failed to initialize terminal")]
    InitError(#[source] io::Error),

    #[error("terminal I/O error")]
    IoError(#[from] io::Error),
}

impl TerminalError {
    pub fn user_friendly_message(&self) -> String {
        match self {
            TerminalError::TooSmall {
                width,
                height,
                min_width,
                min_height,
            } => {
                format!(
                    "终端窗口太小 ({width}x{height})。\n\
                     请调整至至少 {min_width}x{min_height} 个字符。"
                )
            }
            TerminalError::NotATty => "此程序必须在终端中运行。\n\
                 不支持输出重定向或管道。"
                .to_string(),
            TerminalError::RawModeError(_) => "终端原始模式初始化失败。\n\
                 可能需要在合适的终端模拟器中运行。"
                .to_string(),
            TerminalError::SizeError(_) => "无法检测终端大小。\n\
                 请确保在标准终端中运行。"
                .to_string(),
            _ => self.to_string(),
        }
    }
}

#[derive(ThisError, Debug)]
pub enum GeolocationError {
    #[error("cannot reach geolocation service")]
    Unreachable(#[source] NetworkError),

    #[error("failed to parse location data: {0}")]
    ParseError(String),

    #[error("failed after {attempts} retry attempts")]
    RetriesExhausted { attempts: u32 },
}

impl GeolocationError {
    pub fn user_friendly_message(&self) -> String {
        match self {
            GeolocationError::Unreachable(net_err) => match net_err {
                NetworkError::Timeout { timeout_secs, .. } => {
                    format!(
                        "位置检测超时 ({timeout_secs}秒)。请检查网络连接。\n\
                         将使用配置/默认位置。"
                    )
                }
                NetworkError::DnsFailure { .. } => {
                    "无法访问位置服务。请检查 DNS 设置。\n\
                     将使用配置/默认位置。"
                        .to_string()
                }
                NetworkError::ConnectionRefused { .. } => {
                    "位置服务不可用。请稍后重试。\n\
                     将使用配置/默认位置。"
                        .to_string()
                }
                NetworkError::HttpError { status, .. } => {
                    format!(
                        "位置服务返回错误 (HTTP {status})。\n\
                         将使用配置/默认位置。"
                    )
                }
                NetworkError::JsonParse { .. } => "从位置服务接收到无效数据。\n\
                     将使用配置/默认位置。"
                    .to_string(),
                NetworkError::ClientCreation(_) => "网络客户端初始化失败。\n\
                     将使用配置/默认位置。"
                    .to_string(),
                NetworkError::Other(_) => {
                    "无法自动检测位置。请检查网络连接。\n\
                     将使用配置/默认位置。"
                        .to_string()
                }
            },
            GeolocationError::ParseError(_) => "接收到无效的位置数据。\n\
                 将使用配置/默认位置。"
                .to_string(),
            GeolocationError::RetriesExhausted { attempts } => {
                format!(
                    "经过 {attempts} 次尝试后仍无法检测位置。\n\
                     将使用配置/默认位置。"
                )
            }
        }
    }
}
