# TrustComponent SDK for Rust

The official SDK for the TrustComponent API: TrustCaptcha, TrustAnalytics, TrustMonitoring, Trust VAT Checker and the
platform API, in one package. Developed and hosted in the EU.

Requires Rust 1.80 or newer, async (tokio, reqwest); TLS through rustls by default, `native-tls` as a feature.

## Install

```sh
cargo add trustcomponent
```

## Check a TrustCaptcha result

The widget writes a verification token into your form. One call turns it into the result:

```rust
let tc = trustcomponent::TrustComponent::new("ak_…");
let result = tc.captcha.get_verification_result(&form.verification_token).await?;
if result.verification_passed == Some(true) { /* … */ }
```

Create the API key (`ak_…`) in the dashboard under the project's API keys. The key decides which modules the client can use.

## Every module

The client is `module → resource → operation`, with the same names in every language:

```rust
use trustcomponent::apis::*;

tc.analytics.websites.get_websites(analytics_websites_api::GetWebsitesParams { page: None, page_size: None }).await?;
tc.tax_validation.validations.create_validation(tax_validation_validations_api::CreateValidationParams {
    tax_validation_validation_create_params: trustcomponent::models::TaxValidationValidationCreateParams::new("DE".into(), "DE811569869".into()),
}).await?;
```

The full reference of every operation is at https://docs.trustcomponent.com.

## Configuration

```rust
let tc = trustcomponent::TrustComponent::builder("ak_…")
    .timeout(Duration::from_secs(5)).max_retries(1).integration("my-plugin/1.0")
    .build()?;
```

Options: `host`, `timeout`, `proxy`, `max_retries`, `deprecation_warnings`, `integration`, `logger`.

The SDK retries only what the API reserves for retries: `429` with `Retry-After`, `502`, `503`, `504` and a lost
connection (2 retries by default). When the API marks an operation as deprecated, it logs one warning per operation.
Every request names the SDK in `User-Agent` and `TrustComponent-Sdk`, together with the integration you set.

## Errors

Every error is a `trustcomponent::Error` carrying the HTTP status and the stable `code` of the API:
`Error::Api(ApiError)` with `is_invalid_request`, `is_authentication`, `is_permission`, `is_plan_limit`, `is_not_found`, `is_conflict`, `is_gone`, `is_rate_limit`, `is_server`, `is_operation_retired`; `Error::Connection`, `Error::VerificationTokenInvalid`.

```rust
match tc.captcha.get_verification_result(token).await {
    Err(trustcomponent::Error::Api(e)) if e.is_not_found() => { /* e.status, e.code */ }
    other => { /* … */ }
}
```

`OperationRetired` means the operation was switched off after its sunset: update the SDK.

## Support

The GitHub repository of this SDK is a read-only mirror, generated from the TrustComponent API, so pull requests and
issues are not processed there. For questions and bug reports, write to mail@trustcomponent.com or contact support
through https://www.trustcomponent.com.

## License

MIT
