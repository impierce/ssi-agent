//! An in-process stand-in for the parts of the STACKIT KMS API the key store uses, signing with real keys, so that
//! identity-kit's key store suite can verify the signatures.

use crate::{AccessTokens, StackitError, StackitKmsProvider};
use agent_kms::key_provider::settings::{StackitKmsSettings, StackitRegion};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use p256::{ecdsa::signature::Signer as _, pkcs8::EncodePublicKey as _};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use url::Url;
use uuid::Uuid;
use wiremock::{matchers::any, Mock, MockServer, Request, Respond, ResponseTemplate};

const PROJECT: Uuid = Uuid::from_u128(0xa);
const KEY_RING: Uuid = Uuid::from_u128(0xb);
const ACCESS_TOKEN: &str = "access-token";
pub const ES256: &str = "ecdsa_p256_sha256";
pub const SIGNING: &str = "asymmetric_sign_verify";

struct FakeVersion {
    state: &'static str,
    disabled: bool,
    /// `None` for an algorithm the fake does not sign with.
    signing_key: Option<p256::ecdsa::SigningKey>,
}

impl FakeVersion {
    fn public_key_pem(&self) -> Option<String> {
        let der = self.signing_key.as_ref()?.verifying_key().to_public_key_der().unwrap();
        let body = STANDARD.encode(der.as_bytes());
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(64)
            .map(|line| std::str::from_utf8(line).unwrap())
            .collect();
        Some(format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            lines.join("\n")
        ))
    }
}

struct FakeKey {
    id: Uuid,
    display_name: String,
    state: &'static str,
    purpose: String,
    algorithm: String,
    versions: Vec<FakeVersion>,
    /// How many more times the versions are listed as still being created.
    creating_for: usize,
}

impl FakeKey {
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "keyRingId": KEY_RING,
            "displayName": self.display_name,
            "state": self.state,
            "purpose": self.purpose,
            "algorithm": self.algorithm,
            "protection": "software",
            "importOnly": false,
        })
    }

    fn version_json(&self, number: usize) -> Value {
        let version = &self.versions[number - 1];
        json!({
            "keyId": self.id,
            "keyRingId": KEY_RING,
            "number": number,
            "state": if self.creating_for > 0 { "creating" } else { version.state },
            "disabled": version.disabled,
            "publicKey": version.public_key_pem(),
        })
    }
}

#[derive(Default)]
struct State {
    keys: Vec<FakeKey>,
    versions_created: u8,
    key_ring_removed: bool,
    /// How many more requests have their access token refused.
    token_refusals: usize,
    refused: Option<String>,
    /// An error page answered instead of STACKIT's own error.
    page: Option<String>,
    creating_for: usize,
}

impl State {
    fn new_version(&mut self, algorithm: &str) -> FakeVersion {
        self.versions_created += 1;
        FakeVersion {
            state: "active",
            disabled: false,
            signing_key: (algorithm == ES256)
                .then(|| p256::ecdsa::SigningKey::from_slice(&[self.versions_created; 32]).unwrap()),
        }
    }

    fn add_key(&mut self, display_name: &str, algorithm: &str, purpose: &str) -> Uuid {
        let id = Uuid::from_u128(self.keys.len() as u128 + 1);
        let version = self.new_version(algorithm);
        self.keys.push(FakeKey {
            id,
            display_name: display_name.to_string(),
            state: "active",
            purpose: purpose.to_string(),
            algorithm: algorithm.to_string(),
            versions: vec![version],
            creating_for: self.creating_for,
        });
        id
    }

    fn key(&mut self, id: &str) -> Option<&mut FakeKey> {
        let id = Uuid::parse_str(id).ok()?;
        self.keys.iter_mut().find(|key| key.id == id)
    }
}

/// Hands out the one access token the fake accepts, and counts how often it is forgotten.
#[derive(Default)]
pub struct FakeTokens {
    forgotten: AtomicUsize,
}

#[async_trait]
impl AccessTokens for FakeTokens {
    async fn access_token(&self) -> Result<String, StackitError> {
        Ok(ACCESS_TOKEN.to_string())
    }

    async fn forget(&self) {
        self.forgotten.fetch_add(1, Ordering::SeqCst);
    }
}

pub struct FakeStackit {
    server: MockServer,
    state: Arc<Mutex<State>>,
    tokens: Arc<FakeTokens>,
}

impl FakeStackit {
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let state = Arc::<Mutex<State>>::default();
        Mock::given(any())
            .respond_with(Responder(state.clone()))
            .mount(&server)
            .await;
        Self {
            server,
            state,
            tokens: Arc::default(),
        }
    }

    pub fn provider(&self) -> StackitKmsProvider {
        let settings = StackitKmsSettings {
            project_id: PROJECT,
            region: StackitRegion::Eu01,
            key_ring_id: KEY_RING,
            service_account_email: "unicore@sa.stackit.cloud".to_string(),
        };
        StackitKmsProvider::new(
            reqwest::Client::new(),
            &Url::parse(&self.server.uri()).unwrap(),
            &settings,
            self.tokens.clone(),
        )
    }

    /// Adds a key with one active version, as created outside the key store. Returns its id.
    pub fn add_key(&self, display_name: &str, algorithm: &str, purpose: &str) -> Uuid {
        self.state.lock().unwrap().add_key(display_name, algorithm, purpose)
    }

    /// Adds an active version to the key `id`, as rotating it in STACKIT does.
    pub fn add_version(&self, id: Uuid) {
        let mut state = self.state.lock().unwrap();
        let algorithm = state.key(&id.to_string()).unwrap().algorithm.clone();
        let version = state.new_version(&algorithm);
        state.key(&id.to_string()).unwrap().versions.push(version);
    }

    pub fn set_key_state(&self, id: Uuid, key_state: &'static str) {
        self.state.lock().unwrap().key(&id.to_string()).unwrap().state = key_state;
    }

    pub fn set_version_state(&self, id: Uuid, number: usize, version_state: &'static str) {
        self.state.lock().unwrap().key(&id.to_string()).unwrap().versions[number - 1].state = version_state;
    }

    pub fn disable_version(&self, id: Uuid, number: usize) {
        self.state.lock().unwrap().key(&id.to_string()).unwrap().versions[number - 1].disabled = true;
    }

    /// Lists the versions of keys created from now on as still being created, the first `listings` times.
    pub fn create_versions_slowly(&self, listings: usize) {
        self.state.lock().unwrap().creating_for = listings;
    }

    /// Answers every request for the key ring with 404, as when it doesn't exist.
    pub fn remove_key_ring(&self) {
        self.state.lock().unwrap().key_ring_removed = true;
    }

    /// Refuses the access token of the next `requests` requests, as when it was revoked.
    pub fn refuse_tokens(&self, requests: usize) {
        self.state.lock().unwrap().token_refusals = requests;
    }

    /// Answers every request with 502 and the HTML page `page`, as a load balancer in front of STACKIT could.
    pub fn answer_with_page(&self, page: &str) {
        self.state.lock().unwrap().page = Some(page.to_string());
    }

    /// The state of every key, in the order they were created.
    pub fn key_states(&self) -> Vec<&'static str> {
        self.state.lock().unwrap().keys.iter().map(|key| key.state).collect()
    }

    /// Answers every request with 403 and `message`.
    pub fn refuse(&self, message: &str) {
        self.state.lock().unwrap().refused = Some(message.to_string());
    }

    /// How many requests STACKIT received.
    pub async fn requests(&self) -> usize {
        self.server.received_requests().await.unwrap_or_default().len()
    }

    pub fn forgotten_tokens(&self) -> usize {
        self.tokens.forgotten.load(Ordering::SeqCst)
    }
}

struct Responder(Arc<Mutex<State>>);

fn error(status: u16, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "message": message }))
}

impl Respond for Responder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        let bearer = format!("Bearer {ACCESS_TOKEN}");
        let authorized = request
            .headers
            .get("authorization")
            .is_some_and(|value| value.to_str().ok() == Some(bearer.as_str()));
        if !authorized || state.token_refusals > 0 {
            state.token_refusals = state.token_refusals.saturating_sub(1);
            return error(401, "the access token is invalid");
        }
        if let Some(page) = &state.page {
            return ResponseTemplate::new(502).set_body_raw(page.clone(), "text/html");
        }
        if let Some(message) = &state.refused {
            return error(403, message);
        }

        let key_ring = format!("/v1/projects/{PROJECT}/regions/eu01/keyrings/{KEY_RING}");
        let Some(rest) = request.url.path().strip_prefix(&key_ring) else {
            return error(404, "not found");
        };
        if state.key_ring_removed {
            return error(404, "key ring not found");
        }
        let segments: Vec<&str> = rest.split('/').filter(|segment| !segment.is_empty()).collect();
        let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();

        match (request.method.as_str(), segments.as_slice()) {
            ("GET", []) => ResponseTemplate::new(200).set_body_json(json!({ "id": KEY_RING, "displayName": "kms" })),
            ("GET", ["keys"]) => ResponseTemplate::new(200)
                .set_body_json(json!({ "keys": state.keys.iter().map(FakeKey::json).collect::<Vec<_>>() })),
            ("POST", ["keys"]) => {
                let id = state.add_key(
                    body["displayName"].as_str().unwrap_or_default(),
                    body["algorithm"].as_str().unwrap_or_default(),
                    body["purpose"].as_str().unwrap_or_default(),
                );
                ResponseTemplate::new(201).set_body_json(state.key(&id.to_string()).unwrap().json())
            }
            (method, ["keys", id, rest @ ..]) => {
                let Some(key) = state.key(id) else {
                    return error(404, "key not found");
                };
                match (method, rest) {
                    ("GET", []) => ResponseTemplate::new(200).set_body_json(key.json()),
                    ("DELETE", []) => {
                        key.state = "deleted";
                        ResponseTemplate::new(204)
                    }
                    ("GET", ["versions"]) => {
                        let versions: Vec<Value> = (1..=key.versions.len()).map(|n| key.version_json(n)).collect();
                        key.creating_for = key.creating_for.saturating_sub(1);
                        ResponseTemplate::new(200).set_body_json(json!({ "versions": versions }))
                    }
                    ("GET", ["versions", number]) => match number.parse::<usize>() {
                        Ok(number) if (1..=key.versions.len()).contains(&number) => {
                            ResponseTemplate::new(200).set_body_json(key.version_json(number))
                        }
                        _ => error(404, "version not found"),
                    },
                    ("POST", ["versions", number, "sign"]) => {
                        let Some(version) = number
                            .parse::<usize>()
                            .ok()
                            .and_then(|number| key.versions.get(number.wrapping_sub(1)))
                        else {
                            return error(404, "version not found");
                        };
                        let data = STANDARD.decode(body["data"].as_str().unwrap_or_default());
                        match (&version.signing_key, data) {
                            (Some(signing_key), Ok(data))
                                if key.state == "active"
                                    && key.creating_for == 0
                                    && version.state == "active"
                                    && !version.disabled =>
                            {
                                let signature: p256::ecdsa::Signature = signing_key.sign(&data);
                                ResponseTemplate::new(200).set_body_json(json!({
                                    "data": body["data"],
                                    "signature": STANDARD.encode(signature.to_der().as_bytes()),
                                }))
                            }
                            _ => error(400, "the version can't sign"),
                        }
                    }
                    _ => error(404, "not found"),
                }
            }
            _ => error(404, "not found"),
        }
    }
}
