//! Same checks, in the same order, as smoke/smoke.ts. Needs TC_API_KEY, TC_SMOKE_WEBSITE_ID, TC_SMOKE_HEARTBEAT (`<serviceId>/<monitorId>`)
//! and TC_SMOKE_CAPTCHA_TOKEN (a fresh widget token, unused); TC_HOST defaults to the local public gateway.
//! Run with `cargo run --example smoke`.
use std::time::{Duration, SystemTime};

use base64::Engine;
use trustcomponent::apis::{analytics_server_events_api as server_events, analytics_websites_api as websites, monitoring_heartbeats_api as heartbeats, platform_events_api as events};
use trustcomponent::apis::{tax_validation_schedules_api as schedules, tax_validation_validation_results_api as results, tax_validation_validations_api as validations};
use trustcomponent::{models, Error, TrustComponent};

const UNKNOWN: &str = "00000000-0000-4000-8000-000000000000";

fn report(name: &str, outcome: Result<String, Error>, failed: &mut u32) {
    match outcome {
        Ok(result) => println!("ok    {name}: {result}"),
        Err(e) => {
            *failed += 1;
            println!("FAIL  {name}: {e}")
        }
    }
}

fn expected(label: &str, e: &trustcomponent::ApiError) -> String {
    format!("{label} as expected (status {}, code {})", e.status, e.code.clone().unwrap_or_else(|| "-".into()))
}

fn not_found<T>(outcome: Result<T, Error>) -> Result<String, Error> {
    match outcome {
        Err(Error::Api(e)) if e.is_not_found() => Ok(expected("NotFound", &e)),
        Err(e) => Err(e),
        Ok(_) => Err(Error::Other("expected NotFound".into())),
    }
}

fn rate_limited<T>(outcome: Result<T, Error>) -> Result<String, Error> {
    match outcome {
        Err(Error::Api(e)) if e.is_rate_limit() => Ok(expected("RateLimit", &e)),
        Err(e) => Err(e),
        Ok(_) => Err(Error::Other("expected RateLimit".into())),
    }
}

fn token_invalid<T>(outcome: Result<T, Error>) -> Result<String, Error> {
    match outcome {
        Err(Error::VerificationTokenInvalid) => Ok("VerificationTokenInvalid as expected".into()),
        Err(e) => Err(e),
        Ok(_) => Err(Error::Other("expected VerificationTokenInvalid".into())),
    }
}

fn json<T: serde::Serialize>(value: &T, limit: usize) -> String {
    serde_json::to_string(value).unwrap().chars().take(limit).collect()
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} not set"))
}

fn iso(time: SystemTime) -> String {
    let seconds = time.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    httpdate_to_iso(seconds)
}

fn httpdate_to_iso(seconds: u64) -> String {
    let days = seconds / 86_400;
    let (h, m, s) = ((seconds % 86_400) / 3600, (seconds % 3600) / 60, seconds % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}.000Z")
}

#[tokio::main]
async fn main() {
    let host = std::env::var("TC_HOST").unwrap_or_else(|_| "http://localhost:14000".into());
    let tc = TrustComponent::builder(&env("TC_API_KEY")).host(&host).integration("tc-sdk-smoke/1.0").build().unwrap();
    let website_id = env("TC_SMOKE_WEBSITE_ID");
    let heartbeat = env("TC_SMOKE_HEARTBEAT");
    let (service_id, monitor_id) = heartbeat.split_once('/').expect("TC_SMOKE_HEARTBEAT must be <serviceId>/<monitorId>");
    let token = env("TC_SMOKE_CAPTCHA_TOKEN");
    let now = SystemTime::now();
    let from = iso(now - Duration::from_secs(30 * 86_400));
    let to = iso(now);
    let mut failed = 0;

    report("captcha.get_verification_result", tc.captcha.get_verification_result(&token).await.and_then(|r| {
        if r.verification_passed != Some(true) {
            return Err(Error::Other(format!("not passed: {}", json(&r, 300))));
        }
        Ok(format!("passed, verificationId {}, score {:?}", r.verification_id.map(|v| v.to_string()).unwrap_or_default(), r.score))
    }), &mut failed);
    report("captcha.get_verification_result (second fetch)", rate_limited(tc.captcha.get_verification_result(&token).await), &mut failed);
    report("captcha.get_verification_result (malformed token)", token_invalid(tc.captcha.get_verification_result("not-a-token").await), &mut failed);
    let unknown_token = base64::engine::general_purpose::STANDARD.encode(format!(r#"{{"verificationId":"{UNKNOWN}"}}"#));
    report("captcha.get_verification_result (unknown verification)", not_found(tc.captcha.get_verification_result(&unknown_token).await), &mut failed);

    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
    let event_id = uuid::Uuid::from_u128(nanos).to_string();
    let mut event = models::AnalyticsServerEventBatchEventCreateParams::new(event_id, iso(SystemTime::now()), "sdk_smoke_rust".into(), models::AnalyticsEventCategoryEnum::CustomEvents);
    event.user_id = Some("sdk-smoke".into());
    event.properties = Some([("language".to_string(), "rust".to_string())].into());
    let batch = models::AnalyticsServerEventBatchCreateParams::new(website_id.parse().expect("TC_SMOKE_WEBSITE_ID must be a uuid"), iso(SystemTime::now()), true, vec![event]);
    report("analytics.server_events.create_server_event_batch", tc.analytics.server_events.create_server_event_batch(server_events::CreateServerEventBatchParams {
        analytics_server_event_batch_create_params: batch,
    }).await.and_then(|r| if r.accepted == 1 { Ok(format!("accepted {}, dropped {}", r.accepted, r.dropped.len())) } else { Err(Error::Other(json(&r, 300))) }), &mut failed);
    report("analytics.websites.get_websites", tc.analytics.websites.get_websites(websites::GetWebsitesParams { page: None, page_size: None }).await
        .map(|p| format!("{} websites", p.content.len())), &mut failed);
    report("analytics.websites.get_website_stats_overview", tc.analytics.websites.get_website_stats_overview(websites::GetWebsiteStatsOverviewParams {
        website_id: website_id.clone(), from: from.clone(), to: to.clone(), filter: None,
    }).await.map(|o| json(&o, 100)), &mut failed);
    report("analytics.websites.get_website_stats_timeseries", tc.analytics.websites.get_website_stats_timeseries(websites::GetWebsiteStatsTimeseriesParams {
        website_id: website_id.clone(), from: from.clone(), to: to.clone(), granularity: Some(models::AnalyticsStatsGranularityEnum::Day), filter: None,
    }).await.map(|o| format!("{} bytes", json(&o, usize::MAX).len())), &mut failed);
    report("analytics.websites.get_website_stats_breakdowns", tc.analytics.websites.get_website_stats_breakdowns(websites::GetWebsiteStatsBreakdownsParams {
        website_id: website_id.clone(), dimension: models::AnalyticsStatsDimensionEnum::EventName, from: from.clone(), to: to.clone(), filter: None, page: None, page_size: None,
    }).await.map(|o| json(&o, 100)), &mut failed);
    report("analytics.websites.get_website_stats_conversions", tc.analytics.websites.get_website_stats_conversions(websites::GetWebsiteStatsConversionsParams {
        website_id: website_id.clone(), from, to, filter: None, filter_match_mode: None,
    }).await.map(|o| json(&o, 100)), &mut failed);
    report("analytics.websites.get_website_realtime", tc.analytics.websites.get_website_realtime(websites::GetWebsiteRealtimeParams { website_id }).await
        .map(|o| json(&o, 100)), &mut failed);

    report("monitoring.heartbeats.ping_heartbeat", tc.monitoring.heartbeats.ping_heartbeat(heartbeats::PingHeartbeatParams {
        service_id: service_id.into(), monitor_id: monitor_id.into(),
    }).await.map(|_| "pinged".to_string()), &mut failed);
    report("monitoring.heartbeats.ping_heartbeat (unknown monitor)", not_found(tc.monitoring.heartbeats.ping_heartbeat(heartbeats::PingHeartbeatParams {
        service_id: UNKNOWN.into(), monitor_id: UNKNOWN.into(),
    }).await), &mut failed);

    let mut result_id: Option<String> = None;
    for (country_code, raw_value) in [("DE", "DE811569869"), ("AT", "ATU33864707"), ("GB", "GB220430231"), ("CH", "CHE-116.281.710")] {
        let outcome = tc.tax_validation.validations.create_validation(validations::CreateValidationParams {
            tax_validation_validation_create_params: models::TaxValidationValidationCreateParams::new(country_code.into(), raw_value.into()),
        }).await.map(|v| {
            if result_id.is_none() {
                result_id = v.validation_result_id.flatten().map(|id| id.to_string());
            }
            format!("{:?}, outcome {}, provider {}", v.status, v.outcome.map(|o| format!("{o:?}")).unwrap_or("-".into()), v.provider.map(|p| format!("{p:?}")).unwrap_or("-".into()))
        });
        report(&format!("tax_validation.validations.create_validation ({country_code})"), outcome, &mut failed);
    }
    let item = |country_code: &str, raw_value: &str| models::TaxValidationValidationCreateParams::new(country_code.into(), raw_value.into());
    report("tax_validation.validations.create_validation_batch", tc.tax_validation.validations.create_validation_batch(validations::CreateValidationBatchParams {
        tax_validation_validation_batch_create_params: models::TaxValidationValidationBatchCreateParams::new(vec![item("DE", "DE811569869"), item("AT", "ATU33864707")]),
    }).await.map(|r| format!("{} validations", r.len())), &mut failed);
    report("tax_validation.validation_results.get_validation_result", match result_id {
        Some(id) => tc.tax_validation.validation_results.get_validation_result(results::GetValidationResultParams { validation_result_id: id }).await.map(|r| json(&r, 100)),
        None => Err(Error::Other("no validation returned a result id".into())),
    }, &mut failed);
    report("tax_validation.validation_results.get_validation_result (unknown)", not_found(tc.tax_validation.validation_results.get_validation_result(results::GetValidationResultParams {
        validation_result_id: UNKNOWN.into(),
    }).await), &mut failed);
    report("tax_validation.schedules.create_schedule + delete_schedule", async {
        let s = tc.tax_validation.schedules.create_schedule(schedules::CreateScheduleParams {
            tax_validation_schedule_create_params: models::TaxValidationScheduleCreateParams::new("DE".into(), "DE811569869".into(), models::TaxValidationValidationFrequencyEnum::Monthly),
        }).await?;
        let id = s.schedule_id.to_string();
        tc.tax_validation.schedules.delete_schedule(schedules::DeleteScheduleParams { schedule_id: id.clone() }).await?;
        Ok(format!("created and deleted {id}"))
    }.await, &mut failed);

    report("platform.events.get_events", tc.platform.events.get_events(events::GetEventsParams { from: None, to: None, cursor: None, page: None, page_size: Some(5) }).await
        .map(|p| format!("{} events", p.content.len())), &mut failed);
    println!("{}", if failed > 0 { format!("{failed} FAILED") } else { "all ok".to_string() });
    std::process::exit(if failed > 0 { 1 } else { 0 });
}
