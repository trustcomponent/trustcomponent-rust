//! Official TrustComponent SDK for Rust: TrustCaptcha, TrustAnalytics, TrustMonitoring, Trust VAT Checker and the platform API.
//!
//! ```no_run
//! # async fn run(token: &str) -> Result<(), trustcomponent::Error> {
//! let tc = trustcomponent::TrustComponent::new("ak_…");
//! let result = tc.captcha.get_verification_result(token).await?;
//! # Ok(()) }
//! ```

mod captcha;
mod error;
pub mod generated;
mod middleware;

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;

pub use captcha::{parse_verification_token, VerificationToken};
pub use error::{ApiError, Error};
pub use generated::apis;
pub use generated::models;
pub use generated::modules::{AnalyticsModule, CaptchaModule, MonitoringModule, PlatformModule, TaxValidationModule, VERSION};

use generated::apis::configuration::Configuration;
use middleware::SdkMiddleware;

/// Entry point of the SDK. One client per application: it holds the connection pool.
pub struct TrustComponent {
    pub captcha: CaptchaModule,
    pub analytics: AnalyticsModule,
    pub monitoring: MonitoringModule,
    pub tax_validation: TaxValidationModule,
    pub platform: PlatformModule,
}

impl TrustComponent {
    /// A client with default settings. Panics on an empty key.
    pub fn new(api_key: &str) -> Self {
        Self::builder(api_key).build().expect("TrustComponent client")
    }

    pub fn builder(api_key: &str) -> TrustComponentBuilder {
        TrustComponentBuilder {
            api_key: api_key.trim().to_owned(),
            host: "https://api.trustcomponent.com".to_owned(),
            timeout: Duration::from_secs(10),
            proxy: None,
            max_retries: 2,
            deprecation_warnings: true,
            integration: None,
            logger: Arc::new(|message| eprintln!("{message}")),
        }
    }
}

pub struct TrustComponentBuilder {
    api_key: String,
    host: String,
    timeout: Duration,
    proxy: Option<String>,
    max_retries: u32,
    deprecation_warnings: bool,
    integration: Option<String>,
    logger: Arc<dyn Fn(&str) + Send + Sync>,
}

impl TrustComponentBuilder {
    /// Another API host, e.g. a local stack for tests.
    pub fn host(mut self, host: &str) -> Self { self.host = host.to_owned(); self }
    /// Time for one attempt, from sending until the whole answer arrived.
    pub fn timeout(mut self, timeout: Duration) -> Self { self.timeout = timeout; self }
    /// HTTP proxy, e.g. `http://proxy.example.com:3128`.
    pub fn proxy(mut self, proxy: &str) -> Self { self.proxy = Some(proxy.to_owned()); self }
    /// Retries after 429 with Retry-After, 502/503/504 and lost connections. 0 switches retries off.
    pub fn max_retries(mut self, max_retries: u32) -> Self { self.max_retries = max_retries; self }
    /// Logs a warning once per deprecated operation; off for quiet logs.
    pub fn deprecation_warnings(mut self, enabled: bool) -> Self { self.deprecation_warnings = enabled; self }
    /// Name and version of the plugin or product built on the SDK, e.g. `zola-plugin/1.2.0`.
    pub fn integration(mut self, integration: &str) -> Self { self.integration = Some(integration.to_owned()); self }
    pub fn logger(mut self, logger: impl Fn(&str) + Send + Sync + 'static) -> Self { self.logger = Arc::new(logger); self }

    pub fn build(self) -> Result<TrustComponent, Error> {
        if self.api_key.is_empty() {
            return Err(Error::Other("api_key must not be empty".to_owned()));
        }
        let mut http = reqwest::Client::builder().timeout(self.timeout);
        if let Some(proxy) = &self.proxy {
            http = http.proxy(reqwest::Proxy::all(proxy).map_err(|e| Error::Other(e.to_string()))?);
        }
        let http = http.build().map_err(|e| Error::Other(e.to_string()))?;
        let user_agent = format!("trustcomponent-rust/{VERSION}");
        let client = reqwest_middleware::ClientBuilder::new(http)
            .with(SdkMiddleware {
                api_key: self.api_key.clone(),
                user_agent: user_agent.clone(),
                sdk_header: sdk_header(self.integration.as_deref()),
                max_retries: self.max_retries,
                deprecation_warnings: self.deprecation_warnings,
                logger: self.logger.clone(),
            })
            .build();
        let configuration = Arc::new(Configuration {
            base_path: self.host.trim_end_matches('/').to_owned(),
            user_agent: Some(user_agent),
            client,
            ..Default::default()
        });
        Ok(TrustComponent {
            captcha: CaptchaModule::new(configuration.clone()),
            analytics: AnalyticsModule::new(configuration.clone()),
            monitoring: MonitoringModule::new(configuration.clone()),
            tax_validation: TaxValidationModule::new(configuration.clone()),
            platform: PlatformModule::new(configuration),
        })
    }
}

fn sdk_header(integration: Option<&str>) -> String {
    let mut metadata = serde_json::json!({
        "sdk": "rust", "version": VERSION, "api": generated::modules::API_VERSION,
        "runtime": format!("rust/{}", env!("CARGO_PKG_RUST_VERSION")), "os": std::env::consts::OS,
    });
    if let Some(integration) = integration {
        metadata["integration"] = serde_json::Value::String(integration.to_owned());
    }
    base64::engine::general_purpose::STANDARD.encode(metadata.to_string())
}
