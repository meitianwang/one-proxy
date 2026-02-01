// Kiro account import implementation
// Reads credentials from ~/.aws/sso/cache/kiro-auth-token.json

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// API endpoints
const DESKTOP_AUTH_API: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
const DESKTOP_USAGE_API: &str = "https://codewhisperer.us-east-1.amazonaws.com";
const PROFILE_ARN: &str = "arn:aws:codewhisperer:us-east-1:699475941385:profile/EHGA3GRVQMUK";

/// Kiro quota data returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroQuotaData {
    pub subscription_title: Option<String>,
    pub subscription_type: Option<String>,
    pub usage_limit: Option<i32>,
    pub current_usage: Option<i32>,
    pub days_until_reset: Option<i32>,
    pub free_trial_limit: Option<i32>,
    pub free_trial_usage: Option<i32>,
    pub last_updated: i64,
    pub is_error: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroCredentials {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub region: Option<String>,
    pub provider: Option<String>,
    pub auth_method: Option<String>,
    pub profile_arn: Option<String>,
    // AWS SSO OIDC specific (from device registration file)
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_id_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroImportResult {
    pub email: Option<String>,
    pub credentials: KiroCredentials,
    pub usage_info: Option<KiroUsageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroUsageInfo {
    pub subscription_title: Option<String>,
    pub subscription_type: Option<String>,
    pub usage_limit: Option<i32>,
    pub current_usage: Option<i32>,
    pub days_until_reset: Option<i32>,
    pub free_trial_expiry: Option<f64>,
    pub free_trial_info: Option<KiroFreeTrialInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroFreeTrialInfo {
    pub usage_limit: Option<i32>,
    pub current_usage: Option<i32>,
}

// API response structures
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    profile_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageLimitsResponse {
    days_until_reset: Option<i32>,
    user_info: Option<UserInfo>,
    subscription_info: Option<SubscriptionInfo>,
    usage_breakdown_list: Option<Vec<UsageBreakdown>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    email: Option<String>,
    user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscriptionInfo {
    subscription_title: Option<String>,
    #[serde(rename = "type")]
    subscription_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBreakdown {
    usage_limit: Option<i32>,
    current_usage: Option<i32>,
    free_trial_info: Option<FreeTrialInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FreeTrialInfo {
    usage_limit: Option<i32>,
    current_usage: Option<i32>,
    free_trial_expiry: Option<f64>,
}

/// Get the AWS SSO cache directory path
pub fn get_sso_cache_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".aws/sso/cache")
}

/// Get the kiro-auth-token.json file path
pub fn get_kiro_auth_token_path() -> PathBuf {
    get_sso_cache_dir().join("kiro-auth-token.json")
}

/// AWS SSO OIDC Token Response (for IdC accounts)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdcTokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    token_type: Option<String>,
    id_token: Option<String>,
}

/// Refresh token using Desktop Auth API (for social accounts)
async fn refresh_token_desktop(refresh_token: &str) -> Result<RefreshTokenResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let body = serde_json::json!({
        "refreshToken": refresh_token
    });

    let response = client
        .post(format!("{}/refreshToken", DESKTOP_AUTH_API))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    tracing::info!("Kiro Desktop refresh token response status: {}", status);

    if !status.is_success() {
        return Err(anyhow::anyhow!("Refresh token failed ({}): {}", status, text));
    }

    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("Parse refresh response failed: {}", e))
}

/// Refresh token using AWS SSO OIDC API (for IdC accounts)
async fn refresh_token_idc(
    region: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<IdcTokenResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = format!("https://oidc.{}.amazonaws.com/token", region);

    let body = serde_json::json!({
        "clientId": client_id,
        "clientSecret": client_secret,
        "grantType": "refresh_token",
        "refreshToken": refresh_token
    });

    tracing::info!("Kiro IdC refresh token request to: {}", url);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    tracing::info!("Kiro IdC refresh token response status: {}", status);

    if !status.is_success() {
        return Err(anyhow::anyhow!("IdC refresh token failed ({}): {}", status, text));
    }

    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("Parse IdC refresh response failed: {}", e))
}

/// Get usage limits and user info from Desktop API (for social accounts)
async fn get_usage_limits(access_token: &str) -> Result<UsageLimitsResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = format!(
        "{}/getUsageLimits?isEmailRequired=true&origin=AI_EDITOR&profileArn={}",
        DESKTOP_USAGE_API,
        urlencoding::encode(PROFILE_ARN)
    );

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Accept", "application/json")
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    tracing::info!("Kiro usage limits response status: {}", status);

    if !status.is_success() {
        return Err(anyhow::anyhow!("Get usage limits failed ({}): {}", status, text));
    }

    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("Parse usage response failed: {}", e))
}

/// Get usage limits for IdC accounts (requires special headers)
async fn get_usage_limits_idc(access_token: &str) -> Result<UsageLimitsResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = format!(
        "{}/getUsageLimits?isEmailRequired=true&origin=AI_EDITOR&resourceType=AGENTIC_REQUEST",
        DESKTOP_USAGE_API
    );

    // Generate machine_id and invocation_id
    let machine_id = uuid::Uuid::new_v4().to_string().replace("-", "");
    let invocation_id = uuid::Uuid::new_v4().to_string();
    let kiro_version = "0.6.18";

    let x_amz_user_agent = format!("aws-sdk-js/1.0.0 KiroIDE-{}-{}", kiro_version, machine_id);
    let user_agent = format!(
        "aws-sdk-js/1.0.0 ua/2.1 os/macos lang/js md/nodejs#20.16.0 api/codewhispererruntime#1.0.0 m/E KiroIDE-{}-{}",
        kiro_version, machine_id
    );

    println!("[Kiro] IdC usage limits request to: {}", url);

    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("x-amz-user-agent", &x_amz_user_agent)
        .header("user-agent", &user_agent)
        .header("amz-sdk-invocation-id", &invocation_id)
        .header("amz-sdk-request", "attempt=1; max=1")
        .header("Connection", "close")
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;

    println!("[Kiro] IdC usage limits response status: {}", status);

    if !status.is_success() {
        return Err(anyhow::anyhow!("Get IdC usage limits failed ({}): {}", status, text));
    }

    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("Parse IdC usage response failed: {}", e))
}

/// Import Kiro credentials from kiro-auth-token.json
pub async fn import_local_credentials() -> Result<KiroImportResult> {
    let token_path = get_kiro_auth_token_path();

    if !token_path.exists() {
        return Err(anyhow::anyhow!(
            "Kiro auth token file not found: {:?}\nPlease login with Kiro IDE first.",
            token_path
        ));
    }

    tracing::info!("Reading Kiro credentials from: {:?}", token_path);

    let content = std::fs::read_to_string(&token_path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    let obj = json.as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid kiro-auth-token.json format"))?;

    // Extract main token fields
    let access_token = obj.get("accessToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let refresh_token = obj.get("refreshToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let expires_at = obj.get("expiresAt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let region = obj.get("region")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let provider = obj.get("provider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let auth_method = obj.get("authMethod")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let profile_arn = obj.get("profileArn")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let client_id_hash = obj.get("clientIdHash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Load clientId and clientSecret from device registration file
    let (client_id, client_secret) = if let Some(ref hash) = client_id_hash {
        println!("[Kiro] Loading device registration for hash: {}", hash);
        load_device_registration(hash)?
    } else {
        println!("[Kiro] No clientIdHash found in token file");
        (None, None)
    };

    println!("[Kiro] Loaded credentials - auth_method: {:?}, region: {:?}, client_id present: {}, client_secret present: {}",
        auth_method, region, client_id.is_some(), client_secret.is_some());

    // Try to refresh token and get user info
    let mut email = None;
    let mut usage_info = None;
    let mut final_access_token = access_token.clone();
    let mut final_refresh_token = refresh_token.clone();
    let mut final_profile_arn = profile_arn.clone();

    let is_idc = auth_method.as_deref() == Some("IdC");
    println!("[Kiro] auth method: {:?}, is_idc: {}", auth_method, is_idc);

    if let Some(ref rt) = refresh_token {
        // Choose refresh method based on auth_method
        let refresh_result: Result<(String, String)> = if is_idc {
            // IdC account: use AWS SSO OIDC API
            let region_str = region.as_deref().unwrap_or("us-east-1");
            if let (Some(ref cid), Some(ref csec)) = (&client_id, &client_secret) {
                println!("[Kiro] Using IdC refresh with region: {}", region_str);
                match refresh_token_idc(region_str, cid, csec, rt).await {
                    Ok(resp) => Ok((resp.access_token, resp.refresh_token)),
                    Err(e) => {
                        println!("[Kiro] IdC refresh failed: {}", e);
                        Err(e)
                    }
                }
            } else {
                println!("[Kiro] IdC account missing clientId or clientSecret");
                Err(anyhow::anyhow!("IdC account missing clientId or clientSecret"))
            }
        } else {
            // Social account: use Desktop Auth API
            println!("[Kiro] Using Desktop Auth refresh");
            match refresh_token_desktop(rt).await {
                Ok(resp) => {
                    if resp.profile_arn.is_some() {
                        final_profile_arn = resp.profile_arn;
                    }
                    Ok((resp.access_token, resp.refresh_token))
                }
                Err(e) => {
                    println!("[Kiro] Desktop refresh failed: {}", e);
                    Err(e)
                }
            }
        };

        match refresh_result {
            Ok((new_access_token, new_refresh_token)) => {
                println!("[Kiro] Token refreshed successfully");
                final_access_token = Some(new_access_token.clone());
                final_refresh_token = Some(new_refresh_token);

                // Get usage limits and email - use different API for IdC vs social
                let usage_result = if is_idc {
                    println!("[Kiro] Using IdC usage limits API");
                    get_usage_limits_idc(&new_access_token).await
                } else {
                    get_usage_limits(&new_access_token).await
                };

                match usage_result {
                    Ok(usage_resp) => {
                        println!("[Kiro] Usage limits fetched successfully");

                        // Debug: print the full response
                        println!("[Kiro] subscription_info: {:?}", usage_resp.subscription_info);
                        println!("[Kiro] usage_breakdown_list: {:?}", usage_resp.usage_breakdown_list);
                        println!("[Kiro] days_until_reset: {:?}", usage_resp.days_until_reset);

                        // Extract email
                        email = usage_resp.user_info.as_ref()
                            .and_then(|u| u.email.clone());

                        println!("[Kiro] Email: {:?}", email);

                        // Extract usage info
                        let breakdown = usage_resp.usage_breakdown_list
                            .as_ref()
                            .and_then(|list| list.first());

                        usage_info = Some(KiroUsageInfo {
                            subscription_title: usage_resp.subscription_info.as_ref()
                                .and_then(|s| s.subscription_title.clone()),
                            subscription_type: usage_resp.subscription_info.as_ref()
                                .and_then(|s| s.subscription_type.clone()),
                            usage_limit: breakdown.and_then(|b| b.usage_limit),
                            current_usage: breakdown.and_then(|b| b.current_usage),
                            days_until_reset: usage_resp.days_until_reset,
                            free_trial_expiry: breakdown
                                .and_then(|b| b.free_trial_info.as_ref())
                                .and_then(|f| f.free_trial_expiry),
                            free_trial_info: breakdown
                                .and_then(|b| b.free_trial_info.as_ref())
                                .map(|f| KiroFreeTrialInfo {
                                    usage_limit: f.usage_limit,
                                    current_usage: f.current_usage,
                                }),
                        });
                    }
                    Err(e) => {
                        println!("[Kiro] Failed to get usage limits: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("[Kiro] Failed to refresh token: {}", e);
            }
        }
    }

    Ok(KiroImportResult {
        email,
        credentials: KiroCredentials {
            access_token: final_access_token,
            refresh_token: final_refresh_token,
            expires_at,
            region,
            provider,
            auth_method,
            profile_arn: final_profile_arn,
            client_id,
            client_secret,
            client_id_hash,
        },
        usage_info,
    })
}

/// Load clientId and clientSecret from device registration file
fn load_device_registration(client_id_hash: &str) -> Result<(Option<String>, Option<String>)> {
    let cache_dir = get_sso_cache_dir();
    let reg_path = cache_dir.join(format!("{}.json", client_id_hash));

    if !reg_path.exists() {
        tracing::warn!("Device registration file not found: {:?}", reg_path);
        return Ok((None, None));
    }

    tracing::info!("Loading device registration from: {:?}", reg_path);

    let content = std::fs::read_to_string(&reg_path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    let client_id = json.get("clientId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let client_secret = json.get("clientSecret")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok((client_id, client_secret))
}

/// Start OAuth flow - for Kiro, this imports local credentials
pub async fn start_oauth() -> Result<String> {
    // Import credentials from kiro-auth-token.json
    let result = import_local_credentials().await?;

    // Save to auth file
    let auth_dir = crate::config::resolve_auth_dir();
    let identifier = result.email.as_deref().unwrap_or("default");
    // Sanitize email for filename
    let safe_identifier = identifier.replace(['@', '.'], "_");
    let path = auth_dir.join(format!("kiro_{}.json", safe_identifier));

    let auth_data = serde_json::json!({
        "provider": "kiro",
        "type": "kiro",
        "email": result.email,
        "access_token": result.credentials.access_token,
        "refresh_token": result.credentials.refresh_token,
        "expires_at": result.credentials.expires_at,
        "region": result.credentials.region.unwrap_or_else(|| "us-east-1".to_string()),
        "profile_arn": result.credentials.profile_arn,
        "kiro_provider": result.credentials.provider,
        "auth_method": result.credentials.auth_method,
        "client_id": result.credentials.client_id,
        "client_secret": result.credentials.client_secret,
        "client_id_hash": result.credentials.client_id_hash,
        "subscription_title": result.usage_info.as_ref().and_then(|u| u.subscription_title.clone()),
        "subscription_type": result.usage_info.as_ref().and_then(|u| u.subscription_type.clone()),
        "usage_limit": result.usage_info.as_ref().and_then(|u| u.usage_limit),
        "current_usage": result.usage_info.as_ref().and_then(|u| u.current_usage),
        "days_until_reset": result.usage_info.as_ref().and_then(|u| u.days_until_reset),
        "free_trial_limit": result.usage_info.as_ref().and_then(|u| u.free_trial_info.as_ref()).and_then(|f| f.usage_limit),
        "free_trial_usage": result.usage_info.as_ref().and_then(|u| u.free_trial_info.as_ref()).and_then(|f| f.current_usage),
        "enabled": true,
        "imported_at": chrono::Utc::now().to_rfc3339(),
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(&auth_data)?;
    std::fs::write(&path, content)?;

    tracing::info!("Saved Kiro auth file to {:?}", path);

    Ok(format!("Kiro account imported successfully: {}", identifier))
}

/// Fetch Kiro quota by calling API for fresh data
pub async fn fetch_quota(account_id: &str) -> Result<KiroQuotaData> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if !path.exists() {
        return Ok(KiroQuotaData {
            subscription_title: None,
            subscription_type: None,
            usage_limit: None,
            current_usage: None,
            days_until_reset: None,
            free_trial_limit: None,
            free_trial_usage: None,
            last_updated: chrono::Utc::now().timestamp(),
            is_error: true,
            error_message: Some("Account file not found".to_string()),
        });
    }

    let content = std::fs::read_to_string(&path)?;
    let mut json: serde_json::Value = serde_json::from_str(&content)?;

    // Read credentials from saved auth file
    let refresh_token = json.get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let auth_method = json.get("auth_method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let region = json.get("region")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let client_id = json.get("client_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let client_secret = json.get("client_secret")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let is_idc = auth_method.as_deref() == Some("IdC");
    println!("[Kiro fetch_quota] auth_method: {:?}, is_idc: {}", auth_method, is_idc);

    // Try to refresh token and get fresh quota data
    if let Some(ref rt) = refresh_token {
        let refresh_result: Result<(String, String)> = if is_idc {
            let region_str = region.as_deref().unwrap_or("us-east-1");
            if let (Some(ref cid), Some(ref csec)) = (&client_id, &client_secret) {
                println!("[Kiro fetch_quota] Using IdC refresh with region: {}", region_str);
                match refresh_token_idc(region_str, cid, csec, rt).await {
                    Ok(resp) => Ok((resp.access_token, resp.refresh_token)),
                    Err(e) => {
                        println!("[Kiro fetch_quota] IdC refresh failed: {}", e);
                        Err(e)
                    }
                }
            } else {
                Err(anyhow::anyhow!("IdC account missing clientId or clientSecret"))
            }
        } else {
            println!("[Kiro fetch_quota] Using Desktop Auth refresh");
            match refresh_token_desktop(rt).await {
                Ok(resp) => Ok((resp.access_token, resp.refresh_token)),
                Err(e) => {
                    println!("[Kiro fetch_quota] Desktop refresh failed: {}", e);
                    Err(e)
                }
            }
        };

        match refresh_result {
            Ok((new_access_token, new_refresh_token)) => {
                println!("[Kiro fetch_quota] Token refreshed successfully");

                // Update tokens in saved file
                json["access_token"] = serde_json::json!(new_access_token.clone());
                json["refresh_token"] = serde_json::json!(new_refresh_token);

                // Get fresh usage limits
                let usage_result = if is_idc {
                    println!("[Kiro fetch_quota] Using IdC usage limits API");
                    get_usage_limits_idc(&new_access_token).await
                } else {
                    get_usage_limits(&new_access_token).await
                };

                match usage_result {
                    Ok(usage_resp) => {
                        println!("[Kiro fetch_quota] Usage limits fetched successfully");

                        let breakdown = usage_resp.usage_breakdown_list
                            .as_ref()
                            .and_then(|list| list.first());

                        let subscription_title = usage_resp.subscription_info.as_ref()
                            .and_then(|s| s.subscription_title.clone());
                        let subscription_type = usage_resp.subscription_info.as_ref()
                            .and_then(|s| s.subscription_type.clone());
                        let usage_limit = breakdown.and_then(|b| b.usage_limit);
                        let current_usage = breakdown.and_then(|b| b.current_usage);
                        let days_until_reset = usage_resp.days_until_reset;
                        let free_trial_limit = breakdown
                            .and_then(|b| b.free_trial_info.as_ref())
                            .and_then(|f| f.usage_limit);
                        let free_trial_usage = breakdown
                            .and_then(|b| b.free_trial_info.as_ref())
                            .and_then(|f| f.current_usage);

                        // Update saved file with fresh quota data
                        json["subscription_title"] = serde_json::json!(subscription_title);
                        json["subscription_type"] = serde_json::json!(subscription_type);
                        json["usage_limit"] = serde_json::json!(usage_limit);
                        json["current_usage"] = serde_json::json!(current_usage);
                        json["days_until_reset"] = serde_json::json!(days_until_reset);
                        json["free_trial_limit"] = serde_json::json!(free_trial_limit);
                        json["free_trial_usage"] = serde_json::json!(free_trial_usage);

                        // Save updated file
                        let updated_content = serde_json::to_string_pretty(&json)?;
                        std::fs::write(&path, updated_content)?;

                        return Ok(KiroQuotaData {
                            subscription_title,
                            subscription_type,
                            usage_limit,
                            current_usage,
                            days_until_reset,
                            free_trial_limit,
                            free_trial_usage,
                            last_updated: chrono::Utc::now().timestamp(),
                            is_error: false,
                            error_message: None,
                        });
                    }
                    Err(e) => {
                        println!("[Kiro fetch_quota] Failed to get usage limits: {}", e);
                        return Ok(KiroQuotaData {
                            subscription_title: None,
                            subscription_type: None,
                            usage_limit: None,
                            current_usage: None,
                            days_until_reset: None,
                            free_trial_limit: None,
                            free_trial_usage: None,
                            last_updated: chrono::Utc::now().timestamp(),
                            is_error: true,
                            error_message: Some(format!("Failed to get usage limits: {}", e)),
                        });
                    }
                }
            }
            Err(e) => {
                println!("[Kiro fetch_quota] Failed to refresh token: {}", e);
                return Ok(KiroQuotaData {
                    subscription_title: None,
                    subscription_type: None,
                    usage_limit: None,
                    current_usage: None,
                    days_until_reset: None,
                    free_trial_limit: None,
                    free_trial_usage: None,
                    last_updated: chrono::Utc::now().timestamp(),
                    is_error: true,
                    error_message: Some(format!("Failed to refresh token: {}", e)),
                });
            }
        }
    }

    // Fallback: read from saved file if no refresh token
    let subscription_title = json.get("subscription_title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let subscription_type = json.get("subscription_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let usage_limit = json.get("usage_limit")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let current_usage = json.get("current_usage")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let days_until_reset = json.get("days_until_reset")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let free_trial_limit = json.get("free_trial_limit")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let free_trial_usage = json.get("free_trial_usage")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);

    Ok(KiroQuotaData {
        subscription_title,
        subscription_type,
        usage_limit,
        current_usage,
        days_until_reset,
        free_trial_limit,
        free_trial_usage,
        last_updated: chrono::Utc::now().timestamp(),
        is_error: false,
        error_message: None,
    })
}
