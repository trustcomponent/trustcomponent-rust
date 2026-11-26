use base64::Engine;

use crate::generated::apis::captcha_verifications_api::GetResultParams;
use crate::generated::models::CaptchaVerificationResult;
use crate::generated::modules::CaptchaModule;
use crate::Error;

/// What the TrustCaptcha widget writes into the form.
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationToken {
    pub verification_id: String,
    pub client_failover: bool,
}

impl CaptchaModule {
    /// Returns the result of the verification behind a widget token, the one call a form handler needs. When the
    /// widget could not reach TrustCaptcha (`clientFailover` in the token), the public gateway decides whether the
    /// visitor may pass.
    pub async fn get_verification_result(&self, verification_token: &str) -> Result<CaptchaVerificationResult, Error> {
        let token = parse_verification_token(verification_token)?;
        let verification_id = token.verification_id.parse().map_err(|_| Error::VerificationTokenInvalid)?;
        self.verifications.get_result(GetResultParams { verification_id, client_failover: token.client_failover.then_some(true) }).await
    }
}

pub fn parse_verification_token(verification_token: &str) -> Result<VerificationToken, Error> {
    let data = base64::engine::general_purpose::STANDARD.decode(verification_token.trim()).map_err(|_| Error::VerificationTokenInvalid)?;
    let token: serde_json::Value = serde_json::from_slice(&data).map_err(|_| Error::VerificationTokenInvalid)?;
    let verification_id = token.get("verificationId").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).ok_or(Error::VerificationTokenInvalid)?;
    Ok(VerificationToken { verification_id: verification_id.to_owned(), client_failover: token.get("clientFailover").and_then(|v| v.as_bool()) == Some(true) })
}
